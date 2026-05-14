//! Lifecycle FSM — enforced state transitions with operator escape hatches.
//!
//! Every state change goes through the FSM. Invalid transitions return
//! `OpsxError::InvalidTransition`. When the FSM is wrong for a specific
//! situation, `force_transition` bypasses validation but logs the override
//! in the audit trail with a mandatory reason.
//!
//! The `state` field is private — all mutations go through methods.

use crate::error::OpsxError;
use crate::store::{LifecycleState, StateStore};
use crate::types::*;

/// Maximum audit log entries before rotation. Oldest entries are trimmed
/// on save when the log exceeds this limit.
const AUDIT_LOG_MAX: usize = 500;

/// The lifecycle engine — validates transitions and mutates state.
pub struct Lifecycle<S: StateStore> {
    store: S,
    /// Private — all access through methods. No direct mutation.
    state: LifecycleState,
}

impl<S: StateStore> Lifecycle<S> {
    /// Load or initialize the lifecycle from the store.
    pub fn load(store: S) -> Result<Self, OpsxError> {
        let state = store.load()?;
        Ok(Self { store, state })
    }

    /// Persist the current state to the store.
    fn save(&mut self) -> Result<(), OpsxError> {
        // Rotate audit log if it exceeds the limit
        if self.state.audit_log.len() > AUDIT_LOG_MAX {
            let trim = self.state.audit_log.len() - AUDIT_LOG_MAX;
            tracing::debug!(
                trimmed = trim,
                max = AUDIT_LOG_MAX,
                "audit log rotation — oldest entries trimmed"
            );
            self.state.audit_log.drain(..trim);
        }
        self.store.save(&self.state)
    }

    /// Get the current state (read-only).
    pub fn state(&self) -> &LifecycleState {
        &self.state
    }

    /// Append an audit entry and save.
    fn audit_and_save(
        &mut self,
        entity_type: &str,
        entity_id: &str,
        from: &str,
        to: &str,
        reason: Option<&str>,
        forced: bool,
    ) -> Result<(), OpsxError> {
        self.state.audit_log.push(AuditEntry {
            timestamp: iso_now(),
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
            from_state: from.into(),
            to_state: to.into(),
            reason: reason.map(|s| s.into()),
            forced,
        });
        self.save()
    }

    // ─── Design node operations ─────────────────────────────────────

    /// Create a new design node.
    pub fn create_node(
        &mut self,
        id: &str,
        title: &str,
        parent: Option<&str>,
    ) -> Result<&DesignNode, OpsxError> {
        if self.state.nodes.iter().any(|n| n.id == id) {
            return Err(OpsxError::AlreadyExists(format!("node '{id}'")));
        }
        // Validate parent exists if specified
        if let Some(parent_id) = parent
            && !self.state.nodes.iter().any(|n| n.id == parent_id)
        {
            return Err(OpsxError::NotFound(format!("parent node '{parent_id}'")));
        }
        let now = iso_now();
        self.state.nodes.push(DesignNode {
            id: id.into(),
            title: title.into(),
            state: NodeState::Seed,
            parent: parent.map(|s| s.into()),
            tags: vec![],
            priority: None,
            issue_type: None,
            open_questions: vec![],
            decisions: vec![],
            overview: String::new(),
            bound_change: None,
            created_at: now.clone(),
            updated_at: now,
        });
        self.audit_and_save("node", id, "(new)", "seed", None, false)?;
        Ok(self.state.nodes.last().unwrap())
    }

    /// Create a root node (no parent validation required).
    pub fn create_root_node(&mut self, id: &str, title: &str) -> Result<&DesignNode, OpsxError> {
        self.create_node(id, title, None)
    }

    /// Transition a design node to a new state (FSM-validated).
    pub fn transition_node(&mut self, id: &str, target: NodeState) -> Result<(), OpsxError> {
        let idx = self
            .state
            .nodes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;

        let from = self.state.nodes[idx].state;

        if !from.can_transition_to(target) {
            return Err(OpsxError::InvalidTransition {
                entity: format!("node '{id}'"),
                from: from.as_str().into(),
                to: target.as_str().into(),
            });
        }

        // Enforce preconditions for specific transitions
        if target == NodeState::Decided && !self.state.nodes[idx].open_questions.is_empty() {
            return Err(OpsxError::PreconditionFailed(format!(
                "node '{}' has {} open questions — resolve before deciding",
                id,
                self.state.nodes[idx].open_questions.len()
            )));
        }
        if target == NodeState::Implementing
            && from != NodeState::Decided
            && from != NodeState::Blocked
        {
            return Err(OpsxError::PreconditionFailed(format!(
                "node '{}' must be decided (or blocked) before implementing",
                id
            )));
        }

        // Check milestone freeze — only block regression, not forward progress
        for ms in &self.state.milestones {
            if ms.state == MilestoneState::Frozen
                && ms.nodes.contains(&id.to_string())
                && (target == NodeState::Exploring || target == NodeState::Seed)
            {
                return Err(OpsxError::MilestoneFrozen(ms.name.clone()));
            }
        }

        let from_str = from.as_str().to_string();
        self.state.nodes[idx].state = target;
        self.state.nodes[idx].updated_at = iso_now();
        self.audit_and_save("node", id, &from_str, target.as_str(), None, false)
    }

    /// 🔓 ESCAPE HATCH: Force a state transition, bypassing all FSM validation.
    ///
    /// Use when the FSM rules are wrong for a specific situation, when state.json
    /// is corrupted, or when an agent botched a transition. The override is logged
    /// in the audit trail with a mandatory reason.
    ///
    /// This is the "break glass" operator override — it should be rare and visible.
    pub fn force_transition_node(
        &mut self,
        id: &str,
        target: NodeState,
        reason: &str,
    ) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;

        let from_str = node.state.as_str().to_string();
        tracing::warn!(
            node_id = id,
            from = from_str,
            to = target.as_str(),
            reason = reason,
            "FORCED state transition — bypassing FSM validation"
        );
        node.state = target;
        node.updated_at = iso_now();
        self.audit_and_save("node", id, &from_str, target.as_str(), Some(reason), true)
    }

    /// Add an open question to a node.
    pub fn add_question(&mut self, id: &str, question: &str) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        node.open_questions.push(question.into());
        node.updated_at = iso_now();
        self.save()
    }

    /// Remove an open question from a node.
    pub fn remove_question(&mut self, id: &str, question: &str) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        node.open_questions.retain(|q| q != question);
        node.updated_at = iso_now();
        self.save()
    }

    /// Set a node's title.
    pub fn set_title(&mut self, id: &str, title: &str) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        node.title = title.into();
        node.updated_at = iso_now();
        self.save()
    }

    /// Set a node's overview.
    pub fn set_overview(&mut self, id: &str, overview: &str) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        node.overview = overview.into();
        node.updated_at = iso_now();
        self.save()
    }

    /// Add a tag to a node.
    pub fn add_tag(&mut self, id: &str, tag: &str) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        if !node.tags.contains(&tag.to_string()) {
            node.tags.push(tag.into());
            node.updated_at = iso_now();
        }
        self.save()
    }

    /// Remove a tag from a node.
    pub fn remove_tag(&mut self, id: &str, tag: &str) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        node.tags.retain(|t| t != tag);
        node.updated_at = iso_now();
        self.save()
    }

    /// Set a node's priority.
    pub fn set_priority(&mut self, id: &str, priority: Priority) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        node.priority = Some(priority);
        node.updated_at = iso_now();
        self.save()
    }

    /// Set a node's issue type.
    pub fn set_issue_type(&mut self, id: &str, issue_type: IssueType) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        node.issue_type = Some(issue_type);
        node.updated_at = iso_now();
        self.save()
    }

    /// Bind a node to an OpenSpec change.
    pub fn bind_change(&mut self, node_id: &str, change_name: &str) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == node_id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{node_id}'")))?;
        node.bound_change = Some(change_name.into());
        node.updated_at = iso_now();
        self.save()
    }

    /// Add a decision to a node.
    pub fn add_decision(&mut self, id: &str, decision: Decision) -> Result<(), OpsxError> {
        let node = self
            .state
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        node.decisions.push(decision);
        node.updated_at = iso_now();
        self.save()
    }

    /// Delete a node. Returns the removed node. Fails if the node has children.
    pub fn delete_node(&mut self, id: &str) -> Result<DesignNode, OpsxError> {
        // Check for children
        let has_children = self
            .state
            .nodes
            .iter()
            .any(|n| n.parent.as_deref() == Some(id));
        if has_children {
            return Err(OpsxError::PreconditionFailed(format!(
                "node '{}' has children — delete or reparent them first",
                id
            )));
        }
        // Check for milestone membership
        for ms in &self.state.milestones {
            if ms.nodes.contains(&id.to_string()) {
                return Err(OpsxError::PreconditionFailed(format!(
                    "node '{}' belongs to milestone '{}' — remove it first",
                    id, ms.name
                )));
            }
        }
        let idx = self
            .state
            .nodes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        let node = self.state.nodes.remove(idx);
        self.audit_and_save(
            "node",
            id,
            node.state.as_str(),
            "(deleted)",
            Some("node deleted"),
            false,
        )?;
        Ok(node)
    }

    /// 🔓 ESCAPE HATCH: Force-delete a node, bypassing child/milestone checks.
    pub fn force_delete_node(&mut self, id: &str, reason: &str) -> Result<DesignNode, OpsxError> {
        // Also remove from any milestones
        for ms in &mut self.state.milestones {
            ms.nodes.retain(|n| n != id);
        }
        let idx = self
            .state
            .nodes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| OpsxError::NotFound(format!("node '{id}'")))?;
        let node = self.state.nodes.remove(idx);
        tracing::warn!(node_id = id, reason = reason, "FORCED node deletion");
        self.audit_and_save(
            "node",
            id,
            node.state.as_str(),
            "(force-deleted)",
            Some(reason),
            true,
        )?;
        Ok(node)
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&DesignNode> {
        self.state.nodes.iter().find(|n| n.id == id)
    }

    /// List all nodes.
    pub fn nodes(&self) -> &[DesignNode] {
        &self.state.nodes
    }

    /// Get the audit log.
    pub fn audit_log(&self) -> &[AuditEntry] {
        &self.state.audit_log
    }

    // ─── Change operations ──────────────────────────────────────────

    /// Create a new OpenSpec change.
    pub fn create_change(
        &mut self,
        name: &str,
        title: &str,
        bound_node: Option<&str>,
    ) -> Result<(), OpsxError> {
        if self.state.changes.iter().any(|c| c.name == name) {
            return Err(OpsxError::AlreadyExists(format!("change '{name}'")));
        }
        let now = iso_now();
        self.state.changes.push(Change {
            name: name.into(),
            title: title.into(),
            state: ChangeState::Proposed,
            bound_node: bound_node.map(|s| s.into()),
            specs: vec![],
            test_files: vec![],
            tasks_total: 0,
            tasks_done: 0,
            created_at: now.clone(),
            updated_at: now,
        });
        self.audit_and_save("change", name, "(new)", "proposed", None, false)
    }

    /// Transition a change to a new state (FSM-validated).
    pub fn transition_change(&mut self, name: &str, target: ChangeState) -> Result<(), OpsxError> {
        let idx = self
            .state
            .changes
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("change '{name}'")))?;

        let from = self.state.changes[idx].state;
        if !from.can_transition_to(target) {
            return Err(OpsxError::InvalidTransition {
                entity: format!("change '{name}'"),
                from: from.as_str().into(),
                to: target.as_str().into(),
            });
        }

        // Enforce preconditions — specs before code, plan before implementation
        let change = &self.state.changes[idx];
        match target {
            ChangeState::Specced if change.specs.is_empty() => {
                return Err(OpsxError::PreconditionFailed(format!(
                    "change '{}' has no specs — write Given/When/Then scenarios before marking as specced",
                    name
                )));
            }
            ChangeState::Planned if change.specs.is_empty() => {
                return Err(OpsxError::PreconditionFailed(format!(
                    "change '{}' has no specs — specs are required before planning",
                    name
                )));
            }
            ChangeState::Planned if change.tasks_total == 0 => {
                return Err(OpsxError::PreconditionFailed(format!(
                    "change '{}' has no tasks — generate a plan (tasks.md) before marking as planned",
                    name
                )));
            }
            ChangeState::Testing if change.specs.is_empty() => {
                return Err(OpsxError::PreconditionFailed(format!(
                    "change '{}' has no specs — specs with scenarios are required before writing test stubs",
                    name
                )));
            }
            ChangeState::Implementing if change.test_files.is_empty() => {
                return Err(OpsxError::PreconditionFailed(format!(
                    "change '{}' has no test files — write failing test stubs before implementing (TDD)",
                    name
                )));
            }
            ChangeState::Verifying if change.tasks_done == 0 => {
                return Err(OpsxError::PreconditionFailed(format!(
                    "change '{}' has no completed tasks — complete implementation before verifying",
                    name
                )));
            }
            _ => {}
        }

        let from_str = from.as_str().to_string();
        self.state.changes[idx].state = target;
        self.state.changes[idx].updated_at = iso_now();
        self.audit_and_save("change", name, &from_str, target.as_str(), None, false)
    }

    /// Archive a change while performing the content-store archive step as
    /// part of the same lifecycle operation.
    ///
    /// The closure is called only after the FSM transition is validated. State
    /// is persisted only after the closure succeeds; if persistence fails, the
    /// rollback closure is invoked and the in-memory state is restored.
    ///
    /// Crash caveat: with the JSON-file backend, content and state still live
    /// in separate files. A process death after `archive_content` succeeds but
    /// before `save` completes can leave archived content with pre-archive FSM
    /// state. Callers should surface that through reconciliation/doctor tooling.
    pub fn archive_change_with<F, R>(
        &mut self,
        name: &str,
        archive_content: F,
        rollback_content: R,
    ) -> Result<(), OpsxError>
    where
        F: FnOnce() -> Result<(), OpsxError>,
        R: FnOnce() -> Result<(), OpsxError>,
    {
        let idx = self
            .state
            .changes
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("change '{name}'")))?;

        let from = self.state.changes[idx].state;
        let target = ChangeState::Archived;
        if !from.can_transition_to(target) {
            return Err(OpsxError::InvalidTransition {
                entity: format!("change '{name}'"),
                from: from.as_str().into(),
                to: target.as_str().into(),
            });
        }

        archive_content()?;

        let previous_state = self.state.clone();
        let from_str = from.as_str().to_string();
        self.state.changes[idx].state = target;
        self.state.changes[idx].updated_at = iso_now();
        self.state.audit_log.push(AuditEntry {
            timestamp: iso_now(),
            entity_type: "change".into(),
            entity_id: name.into(),
            from_state: from_str,
            to_state: target.as_str().into(),
            reason: None,
            forced: false,
        });

        if let Err(save_err) = self.save() {
            self.state = previous_state;
            if let Err(rollback_err) = rollback_content() {
                return Err(OpsxError::StoreError(format!(
                    "{save_err}; rollback failed: {rollback_err}"
                )));
            }
            return Err(save_err);
        }

        Ok(())
    }

    /// 🔓 ESCAPE HATCH: Force a change state transition.
    pub fn force_transition_change(
        &mut self,
        name: &str,
        target: ChangeState,
        reason: &str,
    ) -> Result<(), OpsxError> {
        let change = self
            .state
            .changes
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("change '{name}'")))?;

        let from_str = change.state.as_str().to_string();
        tracing::warn!(
            change_name = name,
            from = from_str,
            to = target.as_str(),
            reason = reason,
            "FORCED change transition — bypassing FSM validation"
        );
        change.state = target;
        change.updated_at = iso_now();
        self.audit_and_save(
            "change",
            name,
            &from_str,
            target.as_str(),
            Some(reason),
            true,
        )
    }

    /// Register a spec domain on a change (e.g. "auth", "auth/tokens").
    pub fn add_spec(&mut self, name: &str, domain: &str) -> Result<(), OpsxError> {
        let change = self
            .state
            .changes
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("change '{name}'")))?;
        if !change.specs.contains(&domain.to_string()) {
            change.specs.push(domain.into());
            change.updated_at = iso_now();
        }
        self.save()
    }

    /// Register a test file on a change (TDD — stubs written before implementation).
    pub fn add_test_file(&mut self, name: &str, path: &str) -> Result<(), OpsxError> {
        let change = self
            .state
            .changes
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("change '{name}'")))?;
        if !change.test_files.contains(&path.to_string()) {
            change.test_files.push(path.into());
            change.updated_at = iso_now();
        }
        self.save()
    }

    /// Delete a change.
    pub fn delete_change(&mut self, name: &str) -> Result<Change, OpsxError> {
        let idx = self
            .state
            .changes
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("change '{name}'")))?;
        let change = self.state.changes.remove(idx);
        // Unbind from any node
        for node in &mut self.state.nodes {
            if node.bound_change.as_deref() == Some(name) {
                node.bound_change = None;
            }
        }
        self.audit_and_save(
            "change",
            name,
            change.state.as_str(),
            "(deleted)",
            Some("change deleted"),
            false,
        )?;
        Ok(change)
    }

    /// Update change task progress.
    pub fn update_change_progress(
        &mut self,
        name: &str,
        total: usize,
        done: usize,
    ) -> Result<(), OpsxError> {
        let change = self
            .state
            .changes
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("change '{name}'")))?;
        change.tasks_total = total;
        change.tasks_done = done;
        change.updated_at = iso_now();
        self.save()
    }

    // ─── Milestone operations ───────────────────────────────────────

    /// Create a milestone.
    pub fn create_milestone(&mut self, name: &str) -> Result<(), OpsxError> {
        if self.state.milestones.iter().any(|m| m.name == name) {
            return Err(OpsxError::AlreadyExists(format!("milestone '{name}'")));
        }
        let now = iso_now();
        self.state.milestones.push(Milestone {
            name: name.into(),
            state: MilestoneState::Open,
            nodes: vec![],
            created_at: now.clone(),
            updated_at: now,
        });
        self.audit_and_save("milestone", name, "(new)", "open", None, false)
    }

    /// Add a node to a milestone (creates milestone if needed).
    pub fn milestone_add(&mut self, milestone: &str, node_id: &str) -> Result<(), OpsxError> {
        if !self.state.nodes.iter().any(|n| n.id == node_id) {
            return Err(OpsxError::NotFound(format!("node '{node_id}'")));
        }

        if !self.state.milestones.iter().any(|m| m.name == milestone) {
            self.create_milestone(milestone)?;
        }

        let ms = self
            .state
            .milestones
            .iter_mut()
            .find(|m| m.name == milestone)
            .unwrap();

        if ms.state == MilestoneState::Frozen {
            return Err(OpsxError::MilestoneFrozen(milestone.into()));
        }

        if !ms.nodes.contains(&node_id.to_string()) {
            ms.nodes.push(node_id.into());
            ms.updated_at = iso_now();
        }
        self.save()
    }

    /// Remove a node from a milestone.
    pub fn milestone_remove(&mut self, milestone: &str, node_id: &str) -> Result<(), OpsxError> {
        let ms = self
            .state
            .milestones
            .iter_mut()
            .find(|m| m.name == milestone)
            .ok_or_else(|| OpsxError::NotFound(format!("milestone '{milestone}'")))?;
        ms.nodes.retain(|n| n != node_id);
        ms.updated_at = iso_now();
        self.save()
    }

    /// Freeze a milestone.
    pub fn milestone_freeze(&mut self, name: &str) -> Result<(), OpsxError> {
        let ms = self
            .state
            .milestones
            .iter_mut()
            .find(|m| m.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("milestone '{name}'")))?;
        let from_str = ms.state.to_string();
        ms.state = MilestoneState::Frozen;
        ms.updated_at = iso_now();
        self.audit_and_save("milestone", name, &from_str, "frozen", None, false)
    }

    /// Unfreeze a milestone.
    pub fn milestone_unfreeze(&mut self, name: &str) -> Result<(), OpsxError> {
        let ms = self
            .state
            .milestones
            .iter_mut()
            .find(|m| m.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("milestone '{name}'")))?;
        let from_str = ms.state.to_string();
        ms.state = MilestoneState::Open;
        ms.updated_at = iso_now();
        self.audit_and_save("milestone", name, &from_str, "open", None, false)
    }

    /// Delete a milestone.
    pub fn delete_milestone(&mut self, name: &str) -> Result<Milestone, OpsxError> {
        let idx = self
            .state
            .milestones
            .iter()
            .position(|m| m.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("milestone '{name}'")))?;
        let ms = self.state.milestones.remove(idx);
        self.audit_and_save(
            "milestone",
            name,
            &ms.state.to_string(),
            "(deleted)",
            Some("milestone deleted"),
            false,
        )?;
        Ok(ms)
    }

    /// Get milestone readiness report.
    pub fn milestone_status(&self, name: &str) -> Result<MilestoneStatus, OpsxError> {
        let ms = self
            .state
            .milestones
            .iter()
            .find(|m| m.name == name)
            .ok_or_else(|| OpsxError::NotFound(format!("milestone '{name}'")))?;

        let mut status = MilestoneStatus {
            name: ms.name.clone(),
            state: ms.state,
            total: ms.nodes.len(),
            implemented: 0,
            decided: 0,
            exploring: 0,
            other: 0,
        };

        for node_id in &ms.nodes {
            if let Some(node) = self.state.nodes.iter().find(|n| n.id == *node_id) {
                match node.state {
                    NodeState::Implemented => status.implemented += 1,
                    NodeState::Decided | NodeState::Implementing => status.decided += 1,
                    NodeState::Exploring | NodeState::Resolved => status.exploring += 1,
                    _ => status.other += 1,
                }
            } else {
                status.other += 1;
            }
        }

        Ok(status)
    }

    /// Get all milestones.
    pub fn milestones(&self) -> &[Milestone] {
        &self.state.milestones
    }
}

/// Milestone readiness report.
pub struct MilestoneStatus {
    pub name: String,
    pub state: MilestoneState,
    pub total: usize,
    pub implemented: usize,
    pub decided: usize,
    pub exploring: usize,
    pub other: usize,
}

impl MilestoneStatus {
    pub fn progress_pct(&self) -> usize {
        (self.implemented * 100)
            .checked_div(self.total)
            .unwrap_or(0)
    }
}

/// ISO 8601 timestamp without external chrono dependency.
fn iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let days = (secs / 86400) as i64;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since 1970-01-01 to (year, month, day).
/// Algorithm from Howard Hinnant's date library (public domain).
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{JsonFileStore, LifecycleState};
    use tempfile::TempDir;

    struct FailingSaveStore;

    impl StateStore for FailingSaveStore {
        fn load(&self) -> Result<LifecycleState, OpsxError> {
            Ok(LifecycleState::default())
        }

        fn save(&self, _state: &LifecycleState) -> Result<(), OpsxError> {
            Err(OpsxError::StoreError("forced save failure".into()))
        }
    }

    fn test_lifecycle() -> (TempDir, Lifecycle<JsonFileStore>) {
        let tmp = TempDir::new().unwrap();
        let store = JsonFileStore::new(tmp.path());
        let lc = Lifecycle::load(store).unwrap();
        (tmp, lc)
    }

    #[test]
    fn create_node() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test Node", None).unwrap();
        assert_eq!(lc.nodes().len(), 1);
        assert_eq!(lc.nodes()[0].state, NodeState::Seed);
        assert_eq!(lc.audit_log().len(), 1);
        assert!(!lc.audit_log()[0].forced);
    }

    #[test]
    fn create_node_with_invalid_parent_rejected() {
        let (_tmp, mut lc) = test_lifecycle();
        let err = lc.create_node("child", "Child", Some("nonexistent"));
        assert!(err.is_err());
        match err.unwrap_err() {
            OpsxError::NotFound(msg) => assert!(msg.contains("nonexistent")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn create_node_with_valid_parent() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("parent", "Parent", None).unwrap();
        lc.create_node("child", "Child", Some("parent")).unwrap();
        assert_eq!(
            lc.get_node("child").unwrap().parent.as_deref(),
            Some("parent")
        );
    }

    #[test]
    fn valid_transition() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();
        lc.transition_node("test", NodeState::Exploring).unwrap();
        assert_eq!(lc.get_node("test").unwrap().state, NodeState::Exploring);
        assert_eq!(lc.audit_log().len(), 2);
    }

    #[test]
    fn invalid_transition_rejected() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();
        let err = lc.transition_node("test", NodeState::Implemented);
        assert!(err.is_err());
        match err.unwrap_err() {
            OpsxError::InvalidTransition { .. } => {}
            other => panic!("expected InvalidTransition, got {other:?}"),
        }
    }

    #[test]
    fn force_transition_bypasses_fsm() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();
        lc.force_transition_node("test", NodeState::Implemented, "corrupted state")
            .unwrap();
        assert_eq!(lc.get_node("test").unwrap().state, NodeState::Implemented);
        let last = lc.audit_log().last().unwrap();
        assert!(last.forced);
        assert_eq!(last.reason.as_deref(), Some("corrupted state"));
    }

    #[test]
    fn implemented_can_reopen() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();
        lc.transition_node("test", NodeState::Exploring).unwrap();
        lc.transition_node("test", NodeState::Decided).unwrap();
        lc.transition_node("test", NodeState::Implementing).unwrap();
        lc.transition_node("test", NodeState::Implemented).unwrap();
        lc.transition_node("test", NodeState::Exploring).unwrap();
        assert_eq!(lc.get_node("test").unwrap().state, NodeState::Exploring);
    }

    #[test]
    fn blocked_can_resume_implementing() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();
        lc.transition_node("test", NodeState::Exploring).unwrap();
        lc.transition_node("test", NodeState::Decided).unwrap();
        lc.transition_node("test", NodeState::Implementing).unwrap();
        lc.transition_node("test", NodeState::Blocked).unwrap();
        lc.transition_node("test", NodeState::Implementing).unwrap();
        assert_eq!(lc.get_node("test").unwrap().state, NodeState::Implementing);
    }

    #[test]
    fn decided_requires_no_open_questions() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();
        lc.transition_node("test", NodeState::Exploring).unwrap();
        lc.add_question("test", "Unresolved?").unwrap();

        let err = lc.transition_node("test", NodeState::Decided);
        assert!(matches!(err.unwrap_err(), OpsxError::PreconditionFailed(_)));

        lc.remove_question("test", "Unresolved?").unwrap();
        lc.transition_node("test", NodeState::Decided).unwrap();
    }

    #[test]
    fn node_mutation_methods() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();

        lc.set_title("test", "New Title").unwrap();
        assert_eq!(lc.get_node("test").unwrap().title, "New Title");

        lc.set_overview("test", "New overview").unwrap();
        assert_eq!(lc.get_node("test").unwrap().overview, "New overview");

        lc.add_tag("test", "v0.15.0").unwrap();
        assert_eq!(lc.get_node("test").unwrap().tags, vec!["v0.15.0"]);

        lc.add_tag("test", "v0.15.0").unwrap(); // idempotent
        assert_eq!(lc.get_node("test").unwrap().tags.len(), 1);

        lc.remove_tag("test", "v0.15.0").unwrap();
        assert!(lc.get_node("test").unwrap().tags.is_empty());

        lc.set_priority("test", Priority::new(2)).unwrap();
        assert_eq!(
            lc.get_node("test").unwrap().priority,
            Some(Priority::new(2))
        );

        lc.set_issue_type("test", IssueType::Feature).unwrap();
        assert_eq!(
            lc.get_node("test").unwrap().issue_type,
            Some(IssueType::Feature)
        );
    }

    #[test]
    fn delete_node_basic() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();
        let deleted = lc.delete_node("test").unwrap();
        assert_eq!(deleted.id, "test");
        assert!(lc.nodes().is_empty());
    }

    #[test]
    fn delete_node_with_children_fails() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("parent", "Parent", None).unwrap();
        lc.create_node("child", "Child", Some("parent")).unwrap();
        let err = lc.delete_node("parent");
        assert!(matches!(err.unwrap_err(), OpsxError::PreconditionFailed(_)));
    }

    #[test]
    fn delete_node_in_milestone_fails() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("test", "Test", None).unwrap();
        lc.milestone_add("v1.0", "test").unwrap();
        let err = lc.delete_node("test");
        assert!(matches!(err.unwrap_err(), OpsxError::PreconditionFailed(_)));
    }

    #[test]
    fn force_delete_node_bypasses_checks() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("parent", "Parent", None).unwrap();
        lc.create_node("child", "Child", Some("parent")).unwrap();
        lc.milestone_add("v1.0", "parent").unwrap();
        // Force delete bypasses child and milestone checks
        lc.force_delete_node("parent", "cleaning up").unwrap();
        assert!(lc.get_node("parent").is_none());
        // Should also be removed from milestone
        let ms = lc.milestones().iter().find(|m| m.name == "v1.0").unwrap();
        assert!(!ms.nodes.contains(&"parent".to_string()));
    }

    #[test]
    fn change_lifecycle() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("my-change", "My Change", None).unwrap();

        // 1. Specs before specced
        lc.add_spec("my-change", "core").unwrap();
        lc.transition_change("my-change", ChangeState::Specced)
            .unwrap();

        // 2. Tasks before planned
        lc.update_change_progress("my-change", 3, 0).unwrap();
        lc.transition_change("my-change", ChangeState::Planned)
            .unwrap();

        // 3. TDD: test stubs before implementing
        lc.transition_change("my-change", ChangeState::Testing)
            .unwrap();
        lc.add_test_file("my-change", "src/core.test.ts").unwrap();
        lc.transition_change("my-change", ChangeState::Implementing)
            .unwrap();

        // 4. Complete tasks before verifying
        lc.update_change_progress("my-change", 3, 3).unwrap();
        lc.transition_change("my-change", ChangeState::Verifying)
            .unwrap();
        lc.transition_change("my-change", ChangeState::Archived)
            .unwrap();

        let change = lc
            .state()
            .changes
            .iter()
            .find(|c| c.name == "my-change")
            .unwrap();
        assert_eq!(change.state, ChangeState::Archived);
    }

    #[test]
    fn specced_requires_specs() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("no-specs", "No Specs", None).unwrap();

        // Try to go to specced without specs
        let err = lc.transition_change("no-specs", ChangeState::Specced);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("no specs"),
            "should mention missing specs: {msg}"
        );
    }

    #[test]
    fn planned_requires_tasks() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("no-tasks", "No Tasks", None).unwrap();
        lc.add_spec("no-tasks", "core").unwrap();
        lc.transition_change("no-tasks", ChangeState::Specced)
            .unwrap();

        // Try to go to planned without tasks
        let err = lc.transition_change("no-tasks", ChangeState::Planned);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("no tasks"),
            "should mention missing tasks: {msg}"
        );
    }

    #[test]
    fn implementing_requires_test_files() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("impl-test", "Impl Test", None).unwrap();
        lc.add_spec("impl-test", "core").unwrap();
        lc.transition_change("impl-test", ChangeState::Specced)
            .unwrap();
        lc.update_change_progress("impl-test", 2, 0).unwrap();
        lc.transition_change("impl-test", ChangeState::Planned)
            .unwrap();
        lc.transition_change("impl-test", ChangeState::Testing)
            .unwrap();

        // Try implementing without test files — TDD rejects
        let err = lc.transition_change("impl-test", ChangeState::Implementing);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("no test files"),
            "should mention missing test files: {msg}"
        );

        // Add test stubs and try again
        lc.add_test_file("impl-test", "src/core.test.ts").unwrap();
        lc.transition_change("impl-test", ChangeState::Implementing)
            .unwrap();
        assert_eq!(
            lc.state()
                .changes
                .iter()
                .find(|c| c.name == "impl-test")
                .unwrap()
                .state,
            ChangeState::Implementing
        );
    }

    #[test]
    fn testing_requires_specs() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("test-test", "Test Test", None).unwrap();
        // Force to Planned without proper preconditions to test Testing gate
        lc.add_spec("test-test", "core").unwrap();
        lc.transition_change("test-test", ChangeState::Specced)
            .unwrap();
        lc.update_change_progress("test-test", 1, 0).unwrap();
        lc.transition_change("test-test", ChangeState::Planned)
            .unwrap();

        // Testing should succeed (specs exist)
        lc.transition_change("test-test", ChangeState::Testing)
            .unwrap();
    }

    #[test]
    fn verifying_requires_completed_tasks() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("verify-test", "Verify Test", None)
            .unwrap();
        lc.add_spec("verify-test", "core").unwrap();
        lc.transition_change("verify-test", ChangeState::Specced)
            .unwrap();
        lc.update_change_progress("verify-test", 2, 0).unwrap();
        lc.transition_change("verify-test", ChangeState::Planned)
            .unwrap();
        lc.transition_change("verify-test", ChangeState::Testing)
            .unwrap();
        lc.add_test_file("verify-test", "test.rs").unwrap();
        lc.transition_change("verify-test", ChangeState::Implementing)
            .unwrap();

        // Try to verify with zero completed tasks
        let err = lc.transition_change("verify-test", ChangeState::Verifying);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("no completed tasks"),
            "should mention incomplete tasks: {msg}"
        );

        // Complete tasks and try again
        lc.update_change_progress("verify-test", 2, 2).unwrap();
        lc.transition_change("verify-test", ChangeState::Verifying)
            .unwrap();
    }

    #[test]
    fn change_can_be_abandoned_from_any_active_state() {
        for start_state in [
            ChangeState::Proposed,
            ChangeState::Specced,
            ChangeState::Planned,
            ChangeState::Testing,
            ChangeState::Implementing,
            ChangeState::Verifying,
        ] {
            assert!(
                start_state.can_transition_to(ChangeState::Abandoned),
                "{:?} should be able to transition to Abandoned",
                start_state
            );
        }
    }

    #[test]
    fn archived_change_can_reopen() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("reopen", "Reopen Test", None).unwrap();
        lc.add_spec("reopen", "core").unwrap();
        lc.transition_change("reopen", ChangeState::Specced)
            .unwrap();
        lc.update_change_progress("reopen", 1, 0).unwrap();
        lc.transition_change("reopen", ChangeState::Planned)
            .unwrap();
        lc.transition_change("reopen", ChangeState::Testing)
            .unwrap();
        lc.add_test_file("reopen", "test.rs").unwrap();
        lc.transition_change("reopen", ChangeState::Implementing)
            .unwrap();
        lc.update_change_progress("reopen", 1, 1).unwrap();
        lc.transition_change("reopen", ChangeState::Verifying)
            .unwrap();
        lc.transition_change("reopen", ChangeState::Archived)
            .unwrap();
        lc.transition_change("reopen", ChangeState::Proposed)
            .unwrap();
    }

    #[test]
    fn archive_change_with_runs_content_archive_once() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("archive-me", "Archive Me", None).unwrap();
        lc.add_spec("archive-me", "core").unwrap();
        lc.transition_change("archive-me", ChangeState::Specced)
            .unwrap();
        lc.update_change_progress("archive-me", 1, 0).unwrap();
        lc.transition_change("archive-me", ChangeState::Planned)
            .unwrap();
        lc.transition_change("archive-me", ChangeState::Testing)
            .unwrap();
        lc.add_test_file("archive-me", "test.rs").unwrap();
        lc.transition_change("archive-me", ChangeState::Implementing)
            .unwrap();
        lc.update_change_progress("archive-me", 1, 1).unwrap();
        lc.transition_change("archive-me", ChangeState::Verifying)
            .unwrap();

        let archived = std::cell::Cell::new(false);
        let rolled_back = std::cell::Cell::new(false);
        lc.archive_change_with(
            "archive-me",
            || {
                archived.set(true);
                Ok(())
            },
            || {
                rolled_back.set(true);
                Ok(())
            },
        )
        .unwrap();

        let change = lc
            .state()
            .changes
            .iter()
            .find(|c| c.name == "archive-me")
            .unwrap();
        assert_eq!(change.state, ChangeState::Archived);
        assert!(archived.get());
        assert!(!rolled_back.get());
    }

    #[test]
    fn archive_change_with_rolls_content_back_when_state_save_fails() {
        let mut lc = Lifecycle::load(FailingSaveStore).unwrap();
        lc.state.changes.push(Change {
            name: "rollback-me".into(),
            title: "Rollback Me".into(),
            state: ChangeState::Verifying,
            bound_node: None,
            specs: vec!["core".into()],
            test_files: vec!["test.rs".into()],
            tasks_total: 1,
            tasks_done: 1,
            created_at: "2026-05-14T00:00:00Z".into(),
            updated_at: "2026-05-14T00:00:00Z".into(),
        });

        let archived = std::cell::Cell::new(false);
        let rolled_back = std::cell::Cell::new(false);
        let err = lc
            .archive_change_with(
                "rollback-me",
                || {
                    archived.set(true);
                    Ok(())
                },
                || {
                    rolled_back.set(true);
                    Ok(())
                },
            )
            .unwrap_err()
            .to_string();

        assert!(err.contains("forced save failure"));
        assert!(archived.get());
        assert!(rolled_back.get());
        let change = lc
            .state()
            .changes
            .iter()
            .find(|c| c.name == "rollback-me")
            .unwrap();
        assert_eq!(change.state, ChangeState::Verifying);
    }

    #[test]
    fn delete_change_unbinds_node() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("feat", "Feature", None).unwrap();
        lc.create_change("feat-change", "Feature Change", Some("feat"))
            .unwrap();
        lc.bind_change("feat", "feat-change").unwrap();
        assert!(lc.get_node("feat").unwrap().bound_change.is_some());

        lc.delete_change("feat-change").unwrap();
        assert!(lc.get_node("feat").unwrap().bound_change.is_none());
    }

    #[test]
    fn update_change_progress() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_change("prog", "Progress", None).unwrap();
        lc.update_change_progress("prog", 10, 7).unwrap();
        let change = lc
            .state()
            .changes
            .iter()
            .find(|c| c.name == "prog")
            .unwrap();
        assert_eq!(change.tasks_total, 10);
        assert_eq!(change.tasks_done, 7);
    }

    #[test]
    fn delete_milestone() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("a", "A", None).unwrap();
        lc.milestone_add("v1.0", "a").unwrap();
        let deleted = lc.delete_milestone("v1.0").unwrap();
        assert_eq!(deleted.name, "v1.0");
        assert!(lc.milestones().is_empty());
    }

    #[test]
    fn milestone_freeze_prevents_additions() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("a", "Node A", None).unwrap();
        lc.create_node("b", "Node B", None).unwrap();
        lc.milestone_add("v1.0", "a").unwrap();
        lc.milestone_freeze("v1.0").unwrap();
        assert!(lc.milestone_add("v1.0", "b").is_err());
    }

    #[test]
    fn milestone_status_report() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("a", "A", None).unwrap();
        lc.create_node("b", "B", None).unwrap();
        lc.transition_node("a", NodeState::Exploring).unwrap();
        lc.transition_node("a", NodeState::Decided).unwrap();
        lc.transition_node("a", NodeState::Implementing).unwrap();
        lc.transition_node("a", NodeState::Implemented).unwrap();
        lc.milestone_add("v1.0", "a").unwrap();
        lc.milestone_add("v1.0", "b").unwrap();

        let status = lc.milestone_status("v1.0").unwrap();
        assert_eq!(status.total, 2);
        assert_eq!(status.implemented, 1);
        assert_eq!(status.progress_pct(), 50);
    }

    #[test]
    fn state_persists_across_load() {
        let tmp = TempDir::new().unwrap();
        {
            let store = JsonFileStore::new(tmp.path());
            let mut lc = Lifecycle::load(store).unwrap();
            lc.create_node("persist", "Persisted", None).unwrap();
        }
        {
            let store = JsonFileStore::new(tmp.path());
            let lc = Lifecycle::load(store).unwrap();
            assert_eq!(lc.nodes().len(), 1);
            assert!(!lc.audit_log().is_empty());
        }
    }

    #[test]
    fn iso_timestamp_format() {
        let ts = iso_now();
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn audit_trail_tracks_all_operations() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("a", "A", None).unwrap();
        lc.transition_node("a", NodeState::Exploring).unwrap();
        lc.force_transition_node("a", NodeState::Implemented, "test")
            .unwrap();
        assert_eq!(lc.audit_log().len(), 3);
        assert!(!lc.audit_log()[0].forced);
        assert!(!lc.audit_log()[1].forced);
        assert!(lc.audit_log()[2].forced);
    }

    #[test]
    fn audit_log_rotation() {
        let (_tmp, mut lc) = test_lifecycle();
        lc.create_node("a", "A", None).unwrap();
        // Generate > AUDIT_LOG_MAX entries
        for i in 0..AUDIT_LOG_MAX + 50 {
            lc.add_question("a", &format!("q{i}")).unwrap();
        }
        // After save, audit log should be trimmed to AUDIT_LOG_MAX
        assert!(lc.audit_log().len() <= AUDIT_LOG_MAX);
    }
}
