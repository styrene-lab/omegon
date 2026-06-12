//! Plan registry, projections, evidence, and session-local plan view state.

use serde::{Deserialize, Serialize};

use crate::conversation::{
    CompletedWorkPlan, PlanMode, PlanScope, PlanSource, VisiblePlanState, WorkItem, WorkItemStatus,
};

pub const STALE_PLAN_COPY: &str = "Plan source changed or disappeared; showing last summary. Run /plan sync, /plan rebind, or /plan detach.";

impl PlanStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Backgrounded => "backgrounded",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Detached => "detached",
            Self::Archived => "archived",
            Self::Stale => "stale",
        }
    }

    pub fn workstream_label(&self) -> Option<&'static str> {
        match self {
            Self::Active => Some("active"),
            Self::Backgrounded | Self::Detached => Some("paused"),
            Self::Blocked => Some("blocked"),
            Self::Completed => Some("complete"),
            Self::Stale => Some("waiting"),
            Self::Archived => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PlanBinding {
    pub design_node_id: Option<String>,
    pub openspec_change: Option<String>,
    pub openspec_task_group: Option<String>,
    pub branch: Option<String>,
    pub session_id: Option<String>,
    pub external_task_refs: Vec<ExternalTaskRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct ExternalTaskRef {
    pub system: String,
    pub board_id: Option<String>,
    pub task_id: String,
    pub external_refs: Vec<String>,
}

impl PlanBinding {
    pub fn session_plan_id() -> String {
        "session:current".to_string()
    }

    pub fn openspec_plan_id(change: &str, group: Option<&str>) -> String {
        match group {
            Some(group) if !group.trim().is_empty() => {
                format!("openspec:{}:group:{}", change.trim(), group.trim())
            }
            _ => format!("openspec:{}", change.trim()),
        }
    }

    pub fn design_plan_id(node_id: &str) -> String {
        format!("design:{}", node_id.trim())
    }

    pub fn hybrid_plan_id(change: &str, node_id: &str) -> String {
        format!("hybrid:{}:{}", change.trim(), node_id.trim())
    }

    pub fn branch_plan_id(branch: &str) -> String {
        format!("branch:{}", branch.trim())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    #[default]
    Active,
    Backgrounded,
    Blocked,
    Completed,
    Detached,
    Archived,
    Stale,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskIntent {
    #[default]
    Unspecified,
    Research,
    Design,
    Spec,
    Implementation,
    Validation,
    Documentation,
    Operations,
    Review,
}

impl TaskIntent {
    pub fn infer(text: &str) -> Self {
        let lower = text.to_ascii_lowercase();
        if ["research", "investigate", "compare", "findings", "citation"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Research;
        }
        if ["design", "decision", "resolve question", "architecture"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Design;
        }
        if ["spec", "openspec", "scenario", "requirement"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Spec;
        }
        if ["test", "validate", "validation", "smoke", "lint", "assess"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Validation;
        }
        if ["doc", "changelog", "release note", "guide"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Documentation;
        }
        if ["branch", "tag", "deploy", "worktree", "remote", "push"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Operations;
        }
        if ["review", "inspect", "audit", "blocker"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Review;
        }
        if ["implement", "patch", "edit", "code", "fix"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            return Self::Implementation;
        }
        Self::Unspecified
    }

    pub fn default_completion_policy(self) -> TaskCompletionPolicy {
        match self {
            Self::Research | Self::Design | Self::Validation => {
                TaskCompletionPolicy::EvidenceRequired
            }
            Self::Spec => TaskCompletionPolicy::LifecycleStateReached,
            _ => TaskCompletionPolicy::Manual,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Unspecified => "task",
            Self::Research => "research",
            Self::Design => "design",
            Self::Spec => "spec",
            Self::Implementation => "implementation",
            Self::Validation => "validation",
            Self::Documentation => "documentation",
            Self::Operations => "operations",
            Self::Review => "review",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskCompletionPolicy {
    #[default]
    Manual,
    EvidenceRequired,
    AllSubtasksDone,
    LifecycleStateReached,
    OperatorAccepted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum EvidenceRef {
    Finding(String),
    Citation(String),
    Decision(String),
    ResolvedQuestion(String),
    Spec(String),
    Diff(String),
    Validation(String),
    Documentation(String),
    Operation(String),
    Review(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanEventSource {
    #[default]
    Manual,
    Slash,
    Tool,
    OpenSpecTaskDiff,
    DesignTreeUpdate,
    Cleave,
    Delegate,
    Sentry,
    Git,
    Validation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PlanEvent {
    pub plan_id: String,
    pub task_id: Option<String>,
    pub source: PlanEventSource,
    pub summary: String,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct CompletionLedgerEntry {
    pub plan_id: String,
    pub title: String,
    pub source: PlanSource,
    pub binding: PlanBinding,
    pub summary: String,
    pub item_count: usize,
    pub evidence: Vec<EvidenceRef>,
    pub commits: Vec<String>,
    pub validations: Vec<String>,
    pub lifecycle_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PlanRegistry {
    pub entries: Vec<PlanRegistryEntry>,
    pub tasks: Vec<PlanItemProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PlanRegistryViewState {
    pub entries: Vec<PlanViewEntry>,
}

impl PlanRegistryViewState {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn status_for(&self, plan_id: &str) -> Option<PlanStatus> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.plan_id == plan_id)
            .map(|entry| entry.status)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PlanViewEntry {
    pub plan_id: String,
    pub status: PlanStatus,
    pub last_visible_at: Option<String>,
    pub resume_hint: Option<String>,
    pub dismissed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ResumeCandidate {
    pub plan_id: String,
    pub title: String,
    pub status: PlanStatus,
    pub source: PlanSource,
    pub scope: PlanScope,
    pub rank: u8,
    pub hint: String,
}

impl Default for ResumeCandidate {
    fn default() -> Self {
        Self {
            plan_id: String::new(),
            title: String::new(),
            status: PlanStatus::Stale,
            source: PlanSource::Ephemeral,
            scope: PlanScope::Session,
            rank: u8::MAX,
            hint: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlanReconciliationIssue {
    pub plan_id: String,
    pub kind: PlanReconciliationIssueKind,
    pub message: String,
}

impl Default for PlanReconciliationIssue {
    fn default() -> Self {
        Self {
            plan_id: String::new(),
            kind: PlanReconciliationIssueKind::ProgressDiverged,
            message: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanReconciliationIssueKind {
    MissingTasks,
    ChangedTaskIdentity,
    MissingDesignNode,
    ProgressDiverged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct ProgressSummary {
    pub completed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PlanRegistryEntry {
    pub plan_id: String,
    pub title: String,
    pub scope: PlanScope,
    pub source: PlanSource,
    pub status: PlanStatus,
    pub binding: PlanBinding,
    pub progress: ProgressSummary,
    pub resume_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskBindingDurability {
    #[default]
    None,
    Session,
    Repo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct TaskBindingRecord {
    pub stable_id: String,
    pub task_id: String,
    pub system: String,
    pub external_task_id: String,
    pub source: PlanTaskSourceRef,
    pub revision: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TaskBindingStore {
    pub version: u16,
    pub bindings: Vec<TaskBindingRecord>,
}

impl Default for TaskBindingStore {
    fn default() -> Self {
        Self {
            version: 1,
            bindings: Vec::new(),
        }
    }
}

impl TaskBindingStore {
    pub const FILE_NAME: &'static str = "task-bindings.v1.json";

    pub fn path(repo_root: &std::path::Path) -> std::path::PathBuf {
        repo_root.join(".omegon").join(Self::FILE_NAME)
    }

    pub fn load(repo_root: &std::path::Path) -> anyhow::Result<Self> {
        let path = Self::path(repo_root);
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content)
                .map_err(|err| anyhow::anyhow!("failed to parse {}: {err}", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(anyhow::anyhow!("failed to read {}: {err}", path.display())),
        }
    }

    pub fn save(&self, repo_root: &std::path::Path) -> anyhow::Result<()> {
        let path = Self::path(repo_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(
            &path,
            format!(
                "{content}
"
            ),
        )?;
        Ok(())
    }

    pub fn upsert(&mut self, record: TaskBindingRecord) {
        if let Some(existing) = self.bindings.iter_mut().find(|existing| {
            existing.stable_id == record.stable_id
                && existing.system == record.system
                && existing.external_task_id == record.external_task_id
        }) {
            let created_at = existing.created_at.clone();
            *existing = record;
            existing.created_at = created_at;
        } else {
            self.bindings.push(record);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SessionTaskBinding {
    pub task_id: String,
    pub stable_id: Option<String>,
    pub system: String,
    pub external_task_id: String,
    pub revision: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanTaskStableIdQuality {
    Explicit,
    #[default]
    Fallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PlanTaskSourceRef {
    pub kind: String,
    pub path: Option<String>,
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanTaskMutation {
    BindExternalRef,
    SetStatus,
    AppendEvidence,
    Complete,
    Reopen,
    Detach,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlanItemProjection {
    pub id: String,
    pub stable_id: String,
    pub stable_id_quality: PlanTaskStableIdQuality,
    pub revision: String,
    pub source: PlanTaskSourceRef,
    pub supported_mutations: Vec<PlanTaskMutation>,
    pub plan_id: String,
    pub label: String,
    pub status: WorkItemStatus,
    pub intent: TaskIntent,
    pub completion_policy: TaskCompletionPolicy,
    pub evidence: Vec<EvidenceRef>,
    pub external_task_refs: Vec<ExternalTaskRef>,
    pub writable: bool,
}

impl Default for PlanItemProjection {
    fn default() -> Self {
        Self {
            id: String::new(),
            stable_id: String::new(),
            stable_id_quality: PlanTaskStableIdQuality::Fallback,
            revision: String::new(),
            source: PlanTaskSourceRef::default(),
            supported_mutations: Vec::new(),
            plan_id: String::new(),
            label: String::new(),
            status: WorkItemStatus::Pending,
            intent: TaskIntent::Unspecified,
            completion_policy: TaskCompletionPolicy::Manual,
            evidence: Vec::new(),
            external_task_refs: Vec::new(),
            writable: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAction {
    View,
    Set { items: Vec<String> },
    Approve,
    Execute,
    Advance,
    Complete { index: usize },
    Skip,
    Clear,
}

impl crate::conversation::IntentDocument {
    /// Set the work plan, replacing any existing plan.
    pub fn set_work_plan(&mut self, items: Vec<String>) {
        self.apply_plan_action(PlanAction::Set { items });
    }

    pub fn apply_plan_action(&mut self, action: PlanAction) {
        match action {
            PlanAction::View => self.normalize_visible_plan(),
            PlanAction::Set { items } => self.set_work_plan_inner(items),
            PlanAction::Approve => self.approve_work_plan_inner(),
            PlanAction::Execute => self.execute_work_plan_inner(),
            PlanAction::Advance => self.advance_work_plan_inner(),
            PlanAction::Complete { index } => self.complete_work_item_inner(index),
            PlanAction::Skip => self.skip_work_item_inner(),
            PlanAction::Clear => {
                self.clear_work_plan_inner();
                return;
            }
        }
        self.sync_visible_plan_from_legacy();
    }

    fn set_work_plan_inner(&mut self, items: Vec<String>) {
        self.work_plan = items
            .into_iter()
            .filter_map(|desc| {
                let desc = desc.trim();
                (!desc.is_empty()).then(|| desc.to_string())
            })
            .map(|desc| {
                let intent = TaskIntent::infer(&desc);
                WorkItem {
                    description: desc,
                    status: WorkItemStatus::Pending,
                    intent: Some(intent),
                    completion_policy: intent.default_completion_policy(),
                    evidence: Vec::new(),
                }
            })
            .collect();
        if let Some(first) = self.work_plan.first_mut() {
            first.status = WorkItemStatus::Active;
            self.plan_mode = PlanMode::Planning;
        } else {
            self.plan_mode = PlanMode::Off;
        }
    }

    /// Approve the current work plan without starting execution.
    pub fn approve_work_plan(&mut self) {
        self.apply_plan_action(PlanAction::Approve);
    }

    fn approve_work_plan_inner(&mut self) {
        if !self.work_plan.is_empty() && !self.work_plan_complete() {
            self.plan_mode = PlanMode::Approved;
        }
    }

    /// Move an approved plan into execution.
    pub fn execute_work_plan(&mut self) {
        self.apply_plan_action(PlanAction::Execute);
    }

    fn execute_work_plan_inner(&mut self) {
        if !self.work_plan.is_empty() && !self.work_plan_complete() {
            self.plan_mode = PlanMode::Executing;
            if !self
                .work_plan
                .iter()
                .any(|w| w.status == WorkItemStatus::Active)
                && let Some(next) = self
                    .work_plan
                    .iter_mut()
                    .find(|w| w.status == WorkItemStatus::Pending)
            {
                next.status = WorkItemStatus::Active;
            }
        }
    }

    /// Clear the active work plan and disable the plan gate.
    pub fn clear_work_plan(&mut self) {
        self.apply_plan_action(PlanAction::Clear);
    }

    fn clear_work_plan_inner(&mut self) {
        if self
            .visible_plan
            .as_ref()
            .is_some_and(|plan| plan.scope == PlanScope::Repo)
        {
            self.work_plan.clear();
            self.plan_mode = PlanMode::Off;
            if let Some(plan) = self.visible_plan.as_mut() {
                plan.mode = PlanMode::Off;
                plan.items.clear();
                let plan_id = plan.plan_id.clone();
                self.plan_registry_view.entries.push(PlanViewEntry {
                    plan_id: plan_id.clone(),
                    status: PlanStatus::Detached,
                    last_visible_at: None,
                    resume_hint: Some(
                        "Detached repo-bound plan view; durable artifacts unchanged.".to_string(),
                    ),
                    dismissed: false,
                });
                self.plan_events.push(PlanEvent {
                    plan_id,
                    task_id: None,
                    source: PlanEventSource::Slash,
                    summary: "Detached repo-bound plan view; durable artifacts unchanged."
                        .to_string(),
                    evidence: Vec::new(),
                });
            }
            return;
        }

        self.work_plan.clear();
        self.plan_mode = PlanMode::Off;
        self.visible_plan = None;
    }

    /// Advance the work plan: mark the current active item done and activate the next.
    pub fn advance_work_plan(&mut self) {
        self.apply_plan_action(PlanAction::Advance);
    }

    fn advance_work_plan_inner(&mut self) {
        let active_idx = self
            .work_plan
            .iter()
            .position(|w| w.status == WorkItemStatus::Active);
        if let Some(idx) = active_idx {
            if !self.work_item_completion_allowed(idx) {
                self.plan_events.push(PlanEvent {
                    plan_id: self
                        .visible_plan
                        .as_ref()
                        .map(|plan| plan.plan_id.clone())
                        .unwrap_or_else(PlanBinding::session_plan_id),
                    task_id: None,
                    source: PlanEventSource::Manual,
                    summary: "Completion blocked: evidence is required for this task.".to_string(),
                    evidence: Vec::new(),
                });
                return;
            }
            self.work_plan[idx].status = WorkItemStatus::Done;
            if let Some(next) = self.work_plan.get_mut(idx + 1)
                && next.status == WorkItemStatus::Pending
            {
                next.status = WorkItemStatus::Active;
            }
            self.clear_if_work_plan_complete();
        }
    }

    /// Mark a specific work item by index as done.
    pub fn complete_work_item(&mut self, index: usize) {
        self.apply_plan_action(PlanAction::Complete { index });
    }

    fn complete_work_item_inner(&mut self, index: usize) {
        if !self.work_item_completion_allowed(index) {
            self.plan_events.push(PlanEvent {
                plan_id: self
                    .visible_plan
                    .as_ref()
                    .map(|plan| plan.plan_id.clone())
                    .unwrap_or_else(PlanBinding::session_plan_id),
                task_id: Some(format!(
                    "{}:{}",
                    self.visible_plan
                        .as_ref()
                        .map(|plan| plan.plan_id.clone())
                        .unwrap_or_else(PlanBinding::session_plan_id),
                    index + 1
                )),
                source: PlanEventSource::Manual,
                summary: "Completion blocked: evidence is required for this task.".to_string(),
                evidence: Vec::new(),
            });
            return;
        }
        if let Some(item) = self.work_plan.get_mut(index) {
            item.status = WorkItemStatus::Done;
        }
        // If no active item remains, activate the first pending
        if !self
            .work_plan
            .iter()
            .any(|w| w.status == WorkItemStatus::Active)
            && let Some(next) = self
                .work_plan
                .iter_mut()
                .find(|w| w.status == WorkItemStatus::Pending)
        {
            next.status = WorkItemStatus::Active;
        }
        self.clear_if_work_plan_complete();
    }

    /// Skip the current active item and activate the next.
    pub fn skip_work_item(&mut self) {
        self.apply_plan_action(PlanAction::Skip);
    }

    fn skip_work_item_inner(&mut self) {
        let active_idx = self
            .work_plan
            .iter()
            .position(|w| w.status == WorkItemStatus::Active);
        if let Some(idx) = active_idx {
            self.work_plan[idx].status = WorkItemStatus::Skipped;
            if let Some(next) = self.work_plan.get_mut(idx + 1)
                && next.status == WorkItemStatus::Pending
            {
                next.status = WorkItemStatus::Active;
            }
            self.clear_if_work_plan_complete();
        }
    }

    fn work_item_completion_allowed(&self, index: usize) -> bool {
        let Some(item) = self.work_plan.get(index) else {
            return true;
        };
        !matches!(
            item.completion_policy,
            TaskCompletionPolicy::EvidenceRequired
        ) || !item.evidence.is_empty()
    }

    fn clear_if_work_plan_complete(&mut self) {
        if self.work_plan_complete() {
            self.record_completed_work_plan();
            self.plan_mode = PlanMode::Complete;
        }
    }

    fn normalize_visible_plan(&mut self) {
        self.sync_visible_plan_from_legacy();
    }

    fn sync_visible_plan_from_legacy(&mut self) {
        if self.work_plan.is_empty() {
            if self.plan_mode == PlanMode::Off {
                self.visible_plan = None;
            }
            return;
        }

        self.visible_plan = Some(VisiblePlanState {
            plan_id: self
                .visible_plan
                .as_ref()
                .map(|plan| plan.plan_id.clone())
                .unwrap_or_else(PlanBinding::session_plan_id),
            scope: PlanScope::Session,
            source: PlanSource::Ephemeral,
            binding: self
                .visible_plan
                .as_ref()
                .map(|plan| plan.binding.clone())
                .unwrap_or_default(),
            mode: self.plan_mode,
            items: self.work_plan.clone(),
        });
    }

    fn record_completed_work_plan(&mut self) {
        if self.work_plan.is_empty() {
            return;
        }
        if self
            .completed_work_plans
            .last()
            .is_some_and(|record| record.items == self.work_plan)
        {
            return;
        }
        self.completed_work_plans.push(CompletedWorkPlan {
            items: self.work_plan.clone(),
            completed_turn: self.stats.turns,
        });
        let plan_id = self
            .visible_plan
            .as_ref()
            .map(|plan| plan.plan_id.clone())
            .unwrap_or_else(PlanBinding::session_plan_id);
        let source = self
            .visible_plan
            .as_ref()
            .map(|plan| plan.source)
            .unwrap_or_default();
        let binding = self
            .visible_plan
            .as_ref()
            .map(|plan| plan.binding.clone())
            .unwrap_or_default();
        let title = self
            .work_plan
            .first()
            .map(|item| item.description.clone())
            .unwrap_or_else(|| "Completed plan".to_string());
        self.completion_ledger.push(CompletionLedgerEntry {
            plan_id: plan_id.clone(),
            title,
            source,
            binding,
            summary: format!("Completed {} item(s)", self.work_plan.len()),
            item_count: self.work_plan.len(),
            evidence: Vec::new(),
            commits: Vec::new(),
            validations: Vec::new(),
            lifecycle_refs: Vec::new(),
        });
        self.plan_events.push(PlanEvent {
            plan_id,
            task_id: None,
            source: PlanEventSource::Tool,
            summary: "Plan completed".to_string(),
            evidence: Vec::new(),
        });
        const MAX_COMPLETED_WORK_PLANS: usize = 5;
        let overflow = self
            .completed_work_plans
            .len()
            .saturating_sub(MAX_COMPLETED_WORK_PLANS);
        if overflow > 0 {
            self.completed_work_plans.drain(0..overflow);
        }
    }

    pub fn last_completed_work_plan(&self) -> Option<&CompletedWorkPlan> {
        self.completed_work_plans.last()
    }

    /// True when all work items are terminal (done or skipped).
    pub fn work_plan_complete(&self) -> bool {
        !self.work_plan.is_empty()
            && self
                .work_plan
                .iter()
                .all(|w| matches!(w.status, WorkItemStatus::Done | WorkItemStatus::Skipped))
    }

    /// Render the work plan as a compact one-line summary.
    pub fn work_plan_summary(&self) -> Option<String> {
        if self.work_plan.is_empty() {
            return None;
        }
        let parts: Vec<String> = self
            .work_plan
            .iter()
            .map(|w| format!("{} {}", w.status.icon(), w.description))
            .collect();
        Some(parts.join("  "))
    }

    /// Render the last completed work plan, if one exists.
    pub fn render_last_completed_work_plan(&self) -> Option<String> {
        let record = self.last_completed_work_plan()?;
        let mut lines = vec![
            "Plan mode: complete".to_string(),
            "Last completed work plan.".to_string(),
            format!("Progress: {}/{}", record.items.len(), record.items.len()),
            String::new(),
        ];
        for (idx, item) in record.items.iter().enumerate() {
            lines.push(format!(
                "{}. {} {}",
                idx + 1,
                item.status.icon(),
                item.description
            ));
        }
        Some(lines.join("\n"))
    }

    /// Render the active work plan with mode and approval-gate guidance.
    pub fn render_work_plan(&self) -> String {
        if self.work_plan.is_empty() {
            return "Plan mode: off\nNo active work plan.".into();
        }
        let done = self
            .work_plan
            .iter()
            .filter(|w| matches!(w.status, WorkItemStatus::Done))
            .count();
        let mut lines = vec![
            format!("Plan mode: {}", self.plan_mode.label()),
            self.plan_mode.guidance().to_string(),
            format!("Progress: {done}/{}", self.work_plan.len()),
            String::new(),
        ];
        for (idx, item) in self.work_plan.iter().enumerate() {
            lines.push(format!(
                "{}. {} {}",
                idx + 1,
                item.status.icon(),
                item.description
            ));
        }
        lines.join("\n")
    }

    pub fn work_plan_snapshot_json(&self) -> serde_json::Value {
        self.work_plan_snapshot_json_with_registry_entries(self.plan_registry().entries)
    }

    pub fn work_plan_snapshot_json_with_registry_entries<I>(
        &self,
        registry_entries: I,
    ) -> serde_json::Value
    where
        I: IntoIterator<Item = PlanRegistryEntry>,
    {
        let done = self
            .work_plan
            .iter()
            .filter(|w| matches!(w.status, WorkItemStatus::Done))
            .count();
        let items: Vec<serde_json::Value> = self
            .work_plan
            .iter()
            .map(|item| {
                serde_json::json!({
                    "description": item.description,
                    "status": item.status.label(),
                })
            })
            .collect();

        let plan_status = self
            .visible_plan_registry_entry()
            .map(|entry| entry.status.label())
            .unwrap_or("detached");
        let visible_plan_id = self
            .visible_plan
            .as_ref()
            .map(|plan| plan.plan_id.as_str())
            .unwrap_or("session:current");
        let workstreams: Vec<serde_json::Value> = registry_entries
            .into_iter()
            .filter(|entry| entry.plan_id != visible_plan_id)
            .filter_map(|entry| {
                let status = entry.status.workstream_label()?;
                Some(serde_json::json!({
                    "id": entry.plan_id,
                    "title": entry.title,
                    "status": status,
                    "completed": entry.progress.completed,
                    "total": entry.progress.total,
                }))
            })
            .collect();

        serde_json::json!({
            "mode": self.plan_mode.label(),
            "guidance": self.plan_mode.guidance(),
            "completed": done,
            "total": self.work_plan.len(),
            "items": items,
            "status": plan_status,
            "plan_id": visible_plan_id,
            "scope": self
                .visible_plan
                .as_ref()
                .map(|plan| plan.scope.label())
                .unwrap_or("session"),
            "source": self
                .visible_plan
                .as_ref()
                .map(|plan| plan.source.label())
                .unwrap_or("session"),
            "workstreams": workstreams,
        })
    }
}

impl crate::conversation::IntentDocument {
    pub fn reconcile_plan_registry(
        &self,
        entries: &[PlanRegistryEntry],
    ) -> Vec<PlanReconciliationIssue> {
        let mut issues = Vec::new();
        if let Some(visible) = self.visible_plan.as_ref() {
            if let Some(projected) = entries
                .iter()
                .find(|entry| entry.plan_id == visible.plan_id)
            {
                let runtime_total = visible.items.len();
                if runtime_total > 0
                    && projected.progress.total > 0
                    && runtime_total != projected.progress.total
                {
                    issues.push(PlanReconciliationIssue {
                        plan_id: visible.plan_id.clone(),
                        kind: PlanReconciliationIssueKind::ProgressDiverged,
                        message: format!(
                            "Runtime plan has {runtime_total} item(s), backing projection has {}.",
                            projected.progress.total
                        ),
                    });
                }
            } else if visible.binding.openspec_change.is_some() {
                issues.push(PlanReconciliationIssue {
                    plan_id: visible.plan_id.clone(),
                    kind: PlanReconciliationIssueKind::MissingTasks,
                    message: "OpenSpec-backed plan is missing from the current task projection."
                        .to_string(),
                });
            } else if visible.binding.design_node_id.is_some() {
                issues.push(PlanReconciliationIssue {
                    plan_id: visible.plan_id.clone(),
                    kind: PlanReconciliationIssueKind::MissingDesignNode,
                    message: "Design-bound plan is missing from the current design projection."
                        .to_string(),
                });
            }
        }
        issues
    }

    pub fn promotion_nudges(&self) -> Vec<String> {
        let mut nudges = Vec::new();
        if self.work_plan.len() >= 4
            && self
                .visible_plan
                .as_ref()
                .is_none_or(|plan| plan.scope == PlanScope::Session)
        {
            nudges.push("Session plan has durable-work shape; consider /plan promote openspec:<change> or /plan bind design:<node>.".to_string());
        }
        if self
            .work_plan
            .iter()
            .any(|item| matches!(item.intent, Some(TaskIntent::Research | TaskIntent::Design)))
        {
            nudges.push("Research/design work is question-heavy; consider binding to a design node for evidence and decisions.".to_string());
        }
        if self.work_plan.iter().any(|item| {
            matches!(
                item.intent,
                Some(TaskIntent::Validation | TaskIntent::Operations)
            )
        }) {
            nudges.push("Operations/validation work can record evidence refs for branches, tests, tags, remotes, or assessments.".to_string());
        }
        nudges
    }

    pub fn ranked_resume_candidates(
        &self,
        mut entries: Vec<PlanRegistryEntry>,
    ) -> Vec<ResumeCandidate> {
        if let Some(visible) = self.visible_plan_registry_entry() {
            entries.push(visible);
        }
        let mut candidates: Vec<ResumeCandidate> = entries
            .into_iter()
            .filter(|entry| !matches!(entry.status, PlanStatus::Archived))
            .map(|entry| {
                let rank = match (entry.status, entry.scope, entry.source) {
                    (PlanStatus::Active, _, _) => 0,
                    (PlanStatus::Blocked | PlanStatus::Backgrounded, PlanScope::Repo, _) => 1,
                    (PlanStatus::Stale, _, _) => 3,
                    (PlanStatus::Completed, _, _) => 4,
                    (
                        _,
                        PlanScope::Repo,
                        PlanSource::OpenSpec | PlanSource::Design | PlanSource::Hybrid,
                    ) => 2,
                    _ => 5,
                };
                ResumeCandidate {
                    plan_id: entry.plan_id,
                    title: entry.title,
                    status: entry.status,
                    source: entry.source,
                    scope: entry.scope,
                    rank,
                    hint: entry
                        .resume_hint
                        .unwrap_or_else(|| "resume explicitly with /plan resume".to_string()),
                }
            })
            .collect();
        candidates.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.plan_id.cmp(&b.plan_id)));
        candidates.dedup_by(|a, b| a.plan_id == b.plan_id);
        candidates
    }

    pub fn plan_registry(&self) -> PlanRegistry {
        let mut registry = PlanRegistry::default();
        if let Some(entry) = self.visible_plan_registry_entry() {
            registry.entries.push(entry);
            registry.tasks.extend(self.visible_plan_items());
        }
        if let Some(completed) = self.last_completed_work_plan() {
            let plan_id = "session:last-completed".to_string();
            registry.entries.push(PlanRegistryEntry {
                plan_id: plan_id.clone(),
                title: completed
                    .items
                    .first()
                    .map(|item| item.description.clone())
                    .unwrap_or_else(|| "Last completed session plan".to_string()),
                scope: PlanScope::Session,
                source: PlanSource::Ephemeral,
                status: PlanStatus::Completed,
                binding: PlanBinding::default(),
                progress: ProgressSummary {
                    completed: completed.items.len(),
                    total: completed.items.len(),
                },
                resume_hint: Some("completed session context".to_string()),
            });
            registry.tasks.extend(Self::project_items_for_plan(
                &plan_id,
                &completed.items,
                PlanScope::Session,
                &PlanBinding::default(),
            ));
        }
        registry
    }

    fn project_items_for_plan(
        plan_id: &str,
        items: &[WorkItem],
        scope: PlanScope,
        binding: &PlanBinding,
    ) -> Vec<PlanItemProjection> {
        items
            .iter()
            .enumerate()
            .map(|(idx, item)| PlanItemProjection {
                id: format!("{}:{}", plan_id, idx + 1),
                stable_id: format!("{}:{}", plan_id, idx + 1),
                stable_id_quality: PlanTaskStableIdQuality::Fallback,
                revision: format!("session:{}:{}", plan_id, idx + 1),
                source: PlanTaskSourceRef {
                    kind: "session".to_string(),
                    path: None,
                    anchor: Some(format!("item:{}", idx + 1)),
                },
                supported_mutations: vec![
                    PlanTaskMutation::BindExternalRef,
                    PlanTaskMutation::SetStatus,
                    PlanTaskMutation::AppendEvidence,
                    PlanTaskMutation::Complete,
                    PlanTaskMutation::Reopen,
                ],
                plan_id: plan_id.to_string(),
                label: item.description.clone(),
                status: item.status,
                intent: item
                    .intent
                    .unwrap_or_else(|| TaskIntent::infer(&item.description)),
                completion_policy: item.completion_policy,
                evidence: item.evidence.clone(),
                external_task_refs: binding.external_task_refs.clone(),
                writable: scope == PlanScope::Session,
            })
            .collect()
    }

    pub fn visible_plan_registry_entry(&self) -> Option<PlanRegistryEntry> {
        let plan = self.visible_plan.as_ref()?;
        let completed = plan
            .items
            .iter()
            .filter(|item| matches!(item.status, WorkItemStatus::Done))
            .count();
        let status = if plan.scope == PlanScope::Repo && plan.mode == PlanMode::Off {
            PlanStatus::Detached
        } else if plan.mode == PlanMode::Complete {
            PlanStatus::Completed
        } else {
            PlanStatus::Active
        };

        let status = self
            .plan_registry_view
            .status_for(&plan.plan_id)
            .unwrap_or(status);

        Some(PlanRegistryEntry {
            plan_id: plan.plan_id.clone(),
            title: plan
                .items
                .iter()
                .find(|item| item.status == WorkItemStatus::Active)
                .or_else(|| plan.items.first())
                .map(|item| item.description.clone())
                .unwrap_or_else(|| "Session plan".to_string()),
            scope: plan.scope,
            source: plan.source,
            status,
            binding: plan.binding.clone(),
            progress: ProgressSummary {
                completed,
                total: plan.items.len(),
            },
            resume_hint: Some(format!(
                "{} · {}/{}",
                plan.source.label(),
                completed,
                plan.items.len()
            )),
        })
    }

    pub fn mark_plan_view_status(
        &mut self,
        plan_id: Option<&str>,
        status: PlanStatus,
        summary: &str,
    ) -> String {
        let target = plan_id
            .map(str::to_string)
            .or_else(|| self.visible_plan.as_ref().map(|plan| plan.plan_id.clone()))
            .unwrap_or_else(PlanBinding::session_plan_id);
        self.plan_registry_view.entries.push(PlanViewEntry {
            plan_id: target.clone(),
            status,
            last_visible_at: None,
            resume_hint: Some(summary.to_string()),
            dismissed: false,
        });
        self.plan_events.push(PlanEvent {
            plan_id: target.clone(),
            task_id: None,
            source: PlanEventSource::Slash,
            summary: summary.to_string(),
            evidence: Vec::new(),
        });
        format!(
            "{}
{}",
            summary, target
        )
    }

    pub fn record_plan_promotion(&mut self, target: Option<&str>) -> String {
        let plan_id = self
            .visible_plan
            .as_ref()
            .map(|plan| plan.plan_id.clone())
            .unwrap_or_else(PlanBinding::session_plan_id);
        let target = target.unwrap_or("repo-bound lifecycle work");
        let summary = format!("Plan promotion requested: {target}");
        self.plan_events.push(PlanEvent {
            plan_id: plan_id.clone(),
            task_id: None,
            source: PlanEventSource::Slash,
            summary: summary.clone(),
            evidence: Vec::new(),
        });
        self.plan_registry_view.entries.push(PlanViewEntry {
            plan_id: plan_id.clone(),
            status: PlanStatus::Active,
            last_visible_at: None,
            resume_hint: Some(summary.clone()),
            dismissed: false,
        });
        format!(
            "{summary}
{plan_id}"
        )
    }

    pub fn record_plan_binding_note(&mut self, binding: &str) -> String {
        let plan_id = self
            .visible_plan
            .as_ref()
            .map(|plan| plan.plan_id.clone())
            .unwrap_or_else(PlanBinding::session_plan_id);
        let summary = format!("Plan binding requested: {}", binding.trim());
        self.plan_events.push(PlanEvent {
            plan_id: plan_id.clone(),
            task_id: None,
            source: PlanEventSource::Slash,
            summary: summary.clone(),
            evidence: Vec::new(),
        });
        self.plan_registry_view.entries.push(PlanViewEntry {
            plan_id: plan_id.clone(),
            status: PlanStatus::Active,
            last_visible_at: None,
            resume_hint: Some(summary.clone()),
            dismissed: false,
        });
        format!(
            "{summary}
{plan_id}"
        )
    }

    pub fn switch_visible_plan(&mut self, plan_id: &str) -> String {
        if self
            .visible_plan
            .as_ref()
            .is_some_and(|plan| plan.plan_id == plan_id)
        {
            self.plan_registry_view.entries.push(PlanViewEntry {
                plan_id: plan_id.to_string(),
                status: PlanStatus::Active,
                last_visible_at: None,
                resume_hint: Some("Already visible.".to_string()),
                dismissed: false,
            });
            return format!(
                "Plan already visible.
{plan_id}"
            );
        }
        self.plan_registry_view.entries.push(PlanViewEntry {
            plan_id: plan_id.to_string(),
            status: PlanStatus::Active,
            last_visible_at: None,
            resume_hint: Some(
                "Resume requested; source projection will refresh on next registry build."
                    .to_string(),
            ),
            dismissed: false,
        });
        self.plan_events.push(PlanEvent {
            plan_id: plan_id.to_string(),
            task_id: None,
            source: PlanEventSource::Slash,
            summary: "Plan resume/switch requested.".to_string(),
            evidence: Vec::new(),
        });
        format!(
            "Plan resume requested.
{plan_id}"
        )
    }

    pub fn add_plan_item_evidence(
        &mut self,
        index: usize,
        evidence: EvidenceRef,
    ) -> Option<String> {
        let item = self.work_plan.get_mut(index)?;
        item.evidence.push(evidence.clone());
        let plan_id = self
            .visible_plan
            .as_ref()
            .map(|plan| plan.plan_id.clone())
            .unwrap_or_else(PlanBinding::session_plan_id);
        let task_id = format!("{}:{}", plan_id, index + 1);
        self.plan_events.push(PlanEvent {
            plan_id: plan_id.clone(),
            task_id: Some(task_id.clone()),
            source: PlanEventSource::Manual,
            summary: "Task evidence recorded.".to_string(),
            evidence: vec![evidence],
        });
        if let Some(plan) = self.visible_plan.as_mut() {
            plan.items = self.work_plan.clone();
            plan.mode = self.plan_mode;
        }
        Some(task_id)
    }

    pub fn visible_plan_items(&self) -> Vec<PlanItemProjection> {
        self.visible_plan
            .as_ref()
            .map(|plan| {
                Self::project_items_for_plan(&plan.plan_id, &plan.items, plan.scope, &plan.binding)
            })
            .unwrap_or_default()
    }
}

// Future cleanup note:
//
// This module intentionally owns the plan registry/projection data model and the
// plan-specific `IntentDocument` behavior that used to live directly in
// conversation.rs. The remaining coupling is that these methods are still an
// inherent impl on `IntentDocument`, because the current plan surface reads and
// mutates session fields such as `work_plan`, `visible_plan`, `plan_mode`,
// `plan_registry_view`, `plan_events`, and `completion_ledger`.
//
// A cleaner long-term split is to move those fields behind a dedicated
// `PlanSessionState` member on `IntentDocument`, then make this module operate
// on that state directly. That would let conversation.rs own only intent and
// transcript-derived context while plan.rs owns all plan lifecycle/session view
// behavior without reaching across the full IntentDocument shape.

#[cfg(test)]
mod binding_store_tests {
    use super::*;

    #[test]
    fn task_binding_store_round_trips_and_upserts() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TaskBindingStore::default();
        store.upsert(TaskBindingRecord {
            stable_id: "openspec:demo:task:1.1".to_string(),
            task_id: "openspec:demo:group:Group:1.1".to_string(),
            system: "flynt".to_string(),
            external_task_id: "flynt-1".to_string(),
            source: PlanTaskSourceRef {
                kind: "openspec".to_string(),
                path: Some("openspec/changes/demo/tasks.md".to_string()),
                anchor: Some("1.1".to_string()),
            },
            revision: "source-v1:test".to_string(),
            created_at: "created".to_string(),
            updated_at: "created".to_string(),
        });
        store.save(dir.path()).unwrap();

        let mut loaded = TaskBindingStore::load(dir.path()).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.bindings.len(), 1);
        assert_eq!(loaded.bindings[0].external_task_id, "flynt-1");

        loaded.upsert(TaskBindingRecord {
            stable_id: "openspec:demo:task:1.1".to_string(),
            task_id: "openspec:demo:group:Group:1.1".to_string(),
            system: "flynt".to_string(),
            external_task_id: "flynt-1".to_string(),
            source: PlanTaskSourceRef::default(),
            revision: "source-v1:updated".to_string(),
            created_at: "new-created-ignored".to_string(),
            updated_at: "updated".to_string(),
        });
        assert_eq!(loaded.bindings.len(), 1);
        assert_eq!(loaded.bindings[0].created_at, "created");
        assert_eq!(loaded.bindings[0].updated_at, "updated");
        assert_eq!(loaded.bindings[0].revision, "source-v1:updated");
    }

    #[test]
    fn task_binding_store_missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = TaskBindingStore::load(dir.path()).unwrap();
        assert_eq!(loaded.version, 1);
        assert!(loaded.bindings.is_empty());
    }

    #[test]
    fn task_binding_store_corrupt_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = TaskBindingStore::path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "not json").unwrap();
        let err = TaskBindingStore::load(dir.path()).unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }
}
