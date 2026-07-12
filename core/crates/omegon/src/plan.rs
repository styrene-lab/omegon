//! Plan registry, projections, evidence, and session-local plan view state.

use serde::{Deserialize, Serialize};
use std::path::Path;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PlanSurfaceInputs {
    pub visible: Option<PlanRegistryEntry>,
    pub visible_items: Vec<PlanItemProjection>,
    pub completed_session: Option<ProgressSummary>,
    pub reconciliation_issues: Vec<PlanReconciliationIssue>,
    pub promotion_nudges: Vec<String>,
    pub resume_candidates: Vec<ResumeCandidate>,
    pub lifecycle_entries: Vec<PlanRegistryEntry>,
    pub lifecycle_tasks: Vec<PlanItemProjection>,
}

impl PlanSurfaceInputs {
    pub fn from_intent(intent: &crate::conversation::IntentDocument, repo_root: &Path) -> Self {
        let lifecycle = crate::tools::lifecycle_plan_projection(repo_root);
        let completed_session =
            intent
                .last_completed_work_plan()
                .map(|completed| ProgressSummary {
                    completed: completed.items.len(),
                    total: completed.items.len(),
                });
        Self {
            visible: intent.visible_plan_registry_entry(),
            visible_items: intent.visible_plan_items(),
            completed_session,
            reconciliation_issues: intent.reconcile_plan_registry(&lifecycle.entries),
            promotion_nudges: intent.promotion_nudges(),
            resume_candidates: intent.ranked_resume_candidates(lifecycle.entries.clone()),
            lifecycle_entries: lifecycle.entries,
            lifecycle_tasks: lifecycle.tasks,
        }
    }

    pub fn active_lane(
        &self,
        intent: &crate::conversation::IntentDocument,
    ) -> Option<omegon_traits::PlanLaneProjection> {
        let visible = self.visible.as_ref()?;
        if visible.progress.total == 0
            || matches!(visible.status, PlanStatus::Completed | PlanStatus::Archived)
            || intent.work_plan_complete()
        {
            return None;
        }
        let visible_plan = intent.visible_plan.as_ref();
        Some(omegon_traits::PlanLaneProjection {
            plan_id: visible.plan_id.clone(),
            mode: visible_plan
                .map(|plan| plan.mode.label().to_string())
                .unwrap_or_else(|| intent.plan_mode.label().to_string()),
            guidance: visible_plan
                .map(|plan| plan.mode.guidance().to_string())
                .unwrap_or_else(|| intent.plan_mode.guidance().to_string()),
            status: visible.status.label().to_string(),
            scope: visible.scope.label().to_string(),
            source: visible.source.label().to_string(),
            progress: omegon_traits::PlanProgressProjection {
                completed: visible.progress.completed,
                total: visible.progress.total,
            },
            items: self
                .visible_items
                .iter()
                .map(|item| omegon_traits::PlanItemProjection {
                    id: None,
                    label: item.label.clone(),
                    status: item.status.label().to_string(),
                    intent: None,
                    writable: item.writable,
                })
                .collect(),
        })
    }
}

impl PlanSurfaceInputs {
    pub fn to_agent_projection(
        &self,
        intent: &crate::conversation::IntentDocument,
    ) -> omegon_traits::PlanSurfaceProjection {
        let active = self.active_lane(intent);
        omegon_traits::PlanSurfaceProjection {
            version: 1,
            active,
            workstreams: self
                .lifecycle_entries
                .iter()
                .filter_map(|entry| {
                    let status = entry.status.workstream_label()?;
                    Some(omegon_traits::PlanWorkstreamProjection {
                        id: entry.plan_id.clone(),
                        title: entry.title.clone(),
                        status: status.to_string(),
                        progress: omegon_traits::PlanProgressProjection {
                            completed: entry.progress.completed,
                            total: entry.progress.total,
                        },
                    })
                })
                .collect(),
            completed_session: self.completed_session.as_ref().map(|progress| {
                omegon_traits::PlanProgressProjection {
                    completed: progress.completed,
                    total: progress.total,
                }
            }),
            reconciliation_issues: self
                .reconciliation_issues
                .iter()
                .map(|issue| omegon_traits::PlanReconciliationProjection {
                    plan_id: issue.plan_id.clone(),
                    kind: format!("{:?}", issue.kind),
                    message: issue.message.clone(),
                })
                .collect(),
            promotion_nudges: self.promotion_nudges.clone(),
            resume_candidates: self
                .resume_candidates
                .iter()
                .map(|candidate| omegon_traits::PlanResumeCandidateProjection {
                    plan_id: candidate.plan_id.clone(),
                    status: candidate.status.label().to_string(),
                    source: candidate.source.label().to_string(),
                    hint: candidate.hint.clone(),
                    rank: candidate.rank as usize,
                })
                .collect(),
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
    fn next_ephemeral_plan_id(&mut self) -> String {
        self.next_plan_index = self.next_plan_index.saturating_add(1);
        self.next_plan_index.to_string()
    }

    fn retain_visible_session_plan(&mut self, status: PlanStatus, summary: &str) -> Option<String> {
        let plan = self.visible_plan.take()?;
        if plan.scope != PlanScope::Session {
            self.visible_plan = Some(plan);
            return None;
        }
        let plan_id = plan.plan_id.clone();
        self.retained_session_plans.retain(|saved| saved.plan_id != plan_id);
        self.retained_session_plans.push(plan);
        self.plan_registry_view.entries.retain(|entry| entry.plan_id != plan_id);
        self.plan_registry_view.entries.push(PlanViewEntry {
            plan_id: plan_id.clone(),
            status,
            last_visible_at: None,
            resume_hint: Some(summary.to_string()),
            dismissed: false,
        });
        self.work_plan.clear();
        self.plan_mode = PlanMode::Off;
        self.plan_reconciliation_fingerprint = None;
        self.plan_reconciliation_nudges = 0;
        Some(plan_id)
    }

    /// Explicitly start a new operator task. Prompt arrival alone is not a safe
    /// task-boundary signal: ordinary follow-ups frequently continue the active
    /// plan. Callers use this only for explicit reset/replacement operations.
    pub fn begin_new_operator_task(&mut self) {
        let Some(plan_id) = self.retain_visible_session_plan(
            PlanStatus::Detached,
            "Detached when the operator explicitly started a new task.",
        ) else {
            return;
        };
        self.plan_events.push(PlanEvent {
            plan_id,
            task_id: None,
            source: PlanEventSource::Manual,
            summary: "Detached session plan at explicit operator task boundary.".to_string(),
            evidence: Vec::new(),
        });
    }

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
        let replacing_session = self.visible_plan.as_ref().is_some_and(|plan| {
            plan.scope == PlanScope::Session && !plan.items.is_empty()
        });
        if replacing_session {
            self.retain_visible_session_plan(
                PlanStatus::Detached,
                "Detached when a replacement session plan was created.",
            );
        }
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
        let completed_active = self
            .work_plan
            .get(index)
            .is_some_and(|item| item.status == WorkItemStatus::Active);
        if let Some(item) = self.work_plan.get_mut(index) {
            item.status = WorkItemStatus::Done;
        }

        if completed_active {
            if let Some(next) = self
                .work_plan
                .iter_mut()
                .skip(index.saturating_add(1))
                .find(|w| w.status == WorkItemStatus::Pending)
            {
                next.status = WorkItemStatus::Active;
            }
        } else if !self
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

        let plan_id = self
            .visible_plan
            .as_ref()
            .map(|plan| plan.plan_id.clone())
            .unwrap_or_else(|| self.next_ephemeral_plan_id());
        self.visible_plan = Some(VisiblePlanState {
            plan_id,
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

    /// True when the model should receive the plan in per-turn context.
    pub fn has_active_work_plan_context(&self) -> bool {
        let legacy_plan_active = !self.work_plan.is_empty()
            && !matches!(self.plan_mode, PlanMode::Off | PlanMode::Complete);
        let visible_plan_active = self.visible_plan.as_ref().is_some_and(|plan| {
            !plan.items.is_empty() && !matches!(plan.mode, PlanMode::Off | PlanMode::Complete)
        });
        legacy_plan_active || visible_plan_active
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

    pub fn plan_surface_projection_for_repo(
        &self,
        repo_root: &Path,
    ) -> omegon_traits::PlanSurfaceProjection {
        PlanSurfaceInputs::from_intent(self, repo_root).to_agent_projection(self)
    }

    pub fn work_plan_snapshot_json_for_repo(&self, repo_root: &Path) -> serde_json::Value {
        self.plan_surface_projection_for_repo(repo_root)
            .legacy_snapshot_json()
    }

    pub fn work_plan_snapshot_json_with_registry_entries<I>(
        &self,
        registry_entries: I,
    ) -> serde_json::Value
    where
        I: IntoIterator<Item = PlanRegistryEntry>,
    {
        let mut projection = PlanSurfaceInputs {
            visible: self.visible_plan_registry_entry(),
            visible_items: self.visible_plan_items(),
            ..PlanSurfaceInputs::default()
        };
        projection.lifecycle_entries = registry_entries.into_iter().collect();
        projection.to_agent_projection(self).legacy_snapshot_json()
    }

    fn detached_work_plan_snapshot_json(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": self.plan_mode.label(),
            "guidance": self.plan_mode.guidance(),
            "completed": 0,
            "total": 0,
            "items": [],
            "status": "detached",
            "plan_id": self
                .visible_plan
                .as_ref()
                .map(|plan| plan.plan_id.as_str())
                .unwrap_or("session:current"),
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
            "workstreams": [],
        })
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

    pub fn ranked_resume_candidates(
        &self,
        external_entries: Vec<PlanRegistryEntry>,
    ) -> Vec<ResumeCandidate> {
        let mut candidates = Vec::new();
        if let Some(entry) = self.visible_plan_registry_entry() {
            candidates.push(ResumeCandidate {
                plan_id: entry.plan_id.clone(),
                title: entry.title.clone(),
                status: entry.status,
                source: entry.source,
                scope: entry.scope,
                rank: 0,
                hint: entry
                    .resume_hint
                    .unwrap_or_else(|| "visible plan".to_string()),
            });
        }
        candidates.extend(external_entries.into_iter().map(|entry| {
            let rank = match entry.status {
                PlanStatus::Active | PlanStatus::Backgrounded => 1,
                PlanStatus::Stale | PlanStatus::Blocked | PlanStatus::Detached => 2,
                PlanStatus::Completed => 3,
                PlanStatus::Archived => 4,
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
                    .unwrap_or_else(|| entry.status.label().to_string()),
            }
        }));
        candidates.sort_by_key(|candidate| (candidate.rank, candidate.plan_id.clone()));
        candidates
    }

    pub fn reconcile_plan_registry(
        &self,
        external_entries: &[PlanRegistryEntry],
    ) -> Vec<PlanReconciliationIssue> {
        let Some(visible) = self.visible_plan.as_ref() else {
            return Vec::new();
        };
        if visible.source == PlanSource::OpenSpec
            && !external_entries
                .iter()
                .any(|entry| entry.plan_id == visible.plan_id)
        {
            return vec![PlanReconciliationIssue {
                plan_id: visible.plan_id.clone(),
                kind: PlanReconciliationIssueKind::MissingTasks,
                message: "visible OpenSpec plan is missing from lifecycle projection".to_string(),
            }];
        }
        Vec::new()
    }

    pub fn promotion_nudges(&self) -> Vec<String> {
        if self
            .visible_plan
            .as_ref()
            .is_some_and(|plan| plan.scope == PlanScope::Repo)
        {
            return Vec::new();
        }
        let mut nudges = Vec::new();
        let has_evidence_required = self.work_plan.iter().any(|item| {
            matches!(
                item.completion_policy,
                TaskCompletionPolicy::EvidenceRequired
            )
        });
        let has_design = self
            .work_plan
            .iter()
            .any(|item| matches!(item.intent, Some(TaskIntent::Design)));
        let has_validation = self
            .work_plan
            .iter()
            .any(|item| matches!(item.intent, Some(TaskIntent::Validation)));
        if has_evidence_required || has_design || has_validation {
            nudges.push(
                "durable-work: session plan has research/design/validation work; consider binding it to a design node or OpenSpec change".to_string(),
            );
        }
        if has_design {
            nudges.push("design node: promote design work into the design tree".to_string());
        }
        if has_validation {
            nudges.push(
                "Operations/validation: preserve validation evidence before completing the plan"
                    .to_string(),
            );
        }
        nudges
    }

    pub fn plan_registry(&self) -> PlanRegistry {
        let mut entries = Vec::new();
        if let Some(entry) = self.visible_plan_registry_entry() {
            entries.push(entry);
        }
        for entry in &self.plan_registry_view.entries {
            if entries
                .iter()
                .any(|existing| existing.plan_id == entry.plan_id)
            {
                continue;
            }
            entries.push(PlanRegistryEntry {
                plan_id: entry.plan_id.clone(),
                title: entry
                    .resume_hint
                    .clone()
                    .unwrap_or_else(|| entry.plan_id.clone()),
                scope: PlanScope::Session,
                source: PlanSource::Ephemeral,
                status: entry.status,
                binding: PlanBinding::default(),
                progress: ProgressSummary::default(),
                resume_hint: entry.resume_hint.clone(),
            });
        }
        PlanRegistry {
            entries,
            tasks: self.visible_plan_items(),
        }
    }

    fn project_items_for_plan(
        plan_id: &str,
        items: &[WorkItem],
        scope: PlanScope,
        binding: &PlanBinding,
    ) -> Vec<PlanItemProjection> {
        let writable = scope == PlanScope::Session;
        items
            .iter()
            .enumerate()
            .map(|(idx, item)| PlanItemProjection {
                id: format!("{}:{}", plan_id, idx + 1),
                stable_id: format!("{}:{}", plan_id, idx + 1),
                stable_id_quality: PlanTaskStableIdQuality::Fallback,
                revision: "session".to_string(),
                source: PlanTaskSourceRef {
                    kind: scope.label().to_string(),
                    path: None,
                    anchor: Some((idx + 1).to_string()),
                },
                supported_mutations: if writable {
                    vec![PlanTaskMutation::SetStatus, PlanTaskMutation::Complete]
                } else {
                    Vec::new()
                },
                plan_id: plan_id.to_string(),
                label: item.description.clone(),
                status: item.status,
                intent: item.intent.unwrap_or(TaskIntent::Unspecified),
                completion_policy: item.completion_policy,
                evidence: item.evidence.clone(),
                external_task_refs: binding.external_task_refs.clone(),
                writable,
            })
            .collect()
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

pub fn render_plan_show_text(
    intent: &crate::conversation::IntentDocument,
    repo_root: &Path,
    plan_id: &str,
) -> String {
    let projection = PlanSurfaceInputs::from_intent(intent, repo_root);
    let mut entries = intent.plan_registry().entries;
    entries.extend(projection.lifecycle_entries.clone());
    if let Some(entry) = entries.iter().find(|entry| entry.plan_id == plan_id) {
        let mut lines = vec![format!(
            "Plan {} · {} · {} · {}",
            entry.plan_id,
            entry.status.label(),
            entry.source.label(),
            entry.scope.label()
        )];
        lines.push(format!(
            "Progress: {}/{}",
            entry.progress.completed, entry.progress.total
        ));
        if let Some(hint) = &entry.resume_hint {
            lines.push(format!("Resume: {hint}"));
        }
        let mut items = Vec::new();
        if let Some(visible) = &projection.visible
            && visible.plan_id == entry.plan_id
        {
            items.extend(projection.visible_items.clone());
        }
        items.extend(
            projection
                .lifecycle_tasks
                .iter()
                .filter(|task| task.plan_id == entry.plan_id)
                .cloned(),
        );
        if !items.is_empty() {
            lines.push(String::new());
            lines.push("Items".to_string());
            for item in items {
                lines.push(format!("- {} {}", item.status.icon(), item.label));
            }
        }
        return lines.join("\n");
    }

    format!("Plan {plan_id} · stale\n{STALE_PLAN_COPY}")
}

pub fn render_plan_list_text(
    intent: &crate::conversation::IntentDocument,
    repo_root: &Path,
) -> String {
    let projection = PlanSurfaceInputs::from_intent(intent, repo_root);
    let mut lines = vec!["Plans".to_string()];

    if let Some(visible) = &projection.visible {
        lines.push(String::new());
        lines.push(format!(
            "Visible: {} · {} · {}/{}",
            visible.plan_id,
            visible.status.label(),
            visible.progress.completed,
            visible.progress.total
        ));
        for item in &projection.visible_items {
            lines.push(format!("- {} {}", item.status.icon(), item.label));
        }
    } else if let Some(completed) = &projection.completed_session {
        lines.push(String::new());
        lines.push(format!(
            "Completed session: {}/{}",
            completed.completed, completed.total
        ));
    } else {
        lines.push(String::new());
        lines.push("Visible: none".to_string());
    }

    if !projection.lifecycle_entries.is_empty() {
        lines.push(String::new());
        lines.push("OpenSpec".to_string());
        for entry in &projection.lifecycle_entries {
            lines.push(format!(
                "- {} · {} · {}/{}",
                entry.title,
                entry.status.label(),
                entry.progress.completed,
                entry.progress.total
            ));
        }
    }

    if !projection.lifecycle_tasks.is_empty() {
        lines.push(String::new());
        lines.push("OpenSpec tasks".to_string());
        for task in projection
            .lifecycle_tasks
            .iter()
            .take(crate::tools::PLAN_LIST_VISIBLE_ITEM_LIMIT)
        {
            lines.push(format!(
                "- {} {} · {}",
                task.status.icon(),
                task.label,
                task.plan_id
            ));
        }
        if projection.lifecycle_tasks.len() > crate::tools::PLAN_LIST_VISIBLE_ITEM_LIMIT {
            lines.push(format!(
                "- … and {} more tasks",
                projection.lifecycle_tasks.len() - crate::tools::PLAN_LIST_VISIBLE_ITEM_LIMIT
            ));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::conversation::IntentDocument;

    #[test]
    fn explicit_new_task_retains_unfinished_session_plan_snapshot() {
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["Investigate old task".into(), "Implement fix".into()]);
        let plan_id = intent.visible_plan.as_ref().unwrap().plan_id.clone();

        intent.begin_new_operator_task();

        assert!(intent.work_plan.is_empty());
        assert!(intent.visible_plan.is_none());
        assert_eq!(intent.plan_mode, PlanMode::Off);
        let retained = intent
            .retained_session_plans
            .iter()
            .find(|plan| plan.plan_id == plan_id)
            .expect("detached plan snapshot retained");
        assert_eq!(retained.items.len(), 2);
        assert!(intent.plan_registry_view.entries.iter().any(|entry| {
            entry.plan_id == plan_id && entry.status == PlanStatus::Detached
        }));
    }

    #[test]
    fn ordinary_follow_up_does_not_change_active_session_plan() {
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["Implement fix".into(), "Validate".into()]);
        let before = intent.visible_plan.clone();

        // Prompt arrival has no destructive plan lifecycle hook. Only explicit
        // reset/replacement operations invoke begin_new_operator_task.
        assert_eq!(intent.visible_plan, before);
        assert!(intent.retained_session_plans.is_empty());
    }

    #[test]
    fn replacement_plans_receive_distinct_session_local_indexes() {
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["First plan".into()]);
        let first_id = intent.visible_plan.as_ref().unwrap().plan_id.clone();
        intent.set_work_plan(vec!["Second plan".into()]);
        let second_id = intent.visible_plan.as_ref().unwrap().plan_id.clone();

        assert_eq!(first_id, "1");
        assert_eq!(second_id, "2");
        assert!(intent.retained_session_plans.iter().any(|plan| plan.plan_id == first_id));
    }

    #[test]
    fn independent_sessions_reuse_local_plan_indexes_without_global_collision_claims() {
        let mut first_session = IntentDocument::default();
        let mut second_session = IntentDocument::default();
        first_session.set_work_plan(vec!["First session work".into()]);
        second_session.set_work_plan(vec!["Second session work".into()]);

        assert_eq!(first_session.visible_plan.as_ref().unwrap().plan_id, "1");
        assert_eq!(second_session.visible_plan.as_ref().unwrap().plan_id, "1");
        assert_ne!(
            first_session.visible_plan.as_ref().unwrap().items,
            second_session.visible_plan.as_ref().unwrap().items
        );
    }

    #[test]
    fn explicit_new_task_preserves_repo_plan() {
        let mut intent = IntentDocument::default();
        intent.visible_plan = Some(VisiblePlanState {
            plan_id: PlanBinding::openspec_plan_id("active-change", None),
            scope: PlanScope::Repo,
            source: PlanSource::OpenSpec,
            binding: PlanBinding {
                openspec_change: Some("active-change".into()),
                ..PlanBinding::default()
            },
            mode: PlanMode::Executing,
            items: vec![WorkItem {
                description: "Implement scenario".into(),
                status: WorkItemStatus::Active,
                intent: Some(TaskIntent::Implementation),
                completion_policy: TaskCompletionPolicy::Manual,
                evidence: Vec::new(),
            }],
        });

        intent.begin_new_operator_task();

        assert_eq!(intent.visible_plan.as_ref().unwrap().scope, PlanScope::Repo);
    }

    #[test]
    fn completed_session_plan_leaves_history_without_visible_lane() {
        let dir = tempfile::tempdir().unwrap();
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["finish validation".into()]);
        intent.approve_work_plan();
        intent.execute_work_plan();
        intent.work_plan[0].completion_policy = crate::conversation::TaskCompletionPolicy::Manual;
        intent.complete_work_item(0);

        assert!(intent.work_plan_complete());
        assert!(intent.visible_plan.is_some());
        assert_eq!(intent.plan_mode, crate::conversation::PlanMode::Complete);
        assert_eq!(intent.last_completed_work_plan().unwrap().items.len(), 1);
        let projection = PlanSurfaceInputs::from_intent(&intent, dir.path());
        assert!(projection.active_lane(&intent).is_none());
        assert_eq!(projection.completed_session.unwrap().completed, 1);
    }

    #[test]
    fn render_plan_list_text_includes_visible_and_lifecycle_sections() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("openspec/changes/example/specs/lifecycle")).unwrap();
        std::fs::write(
            repo.join("openspec/changes/example/proposal.md"),
            "# Example\n",
        )
        .unwrap();
        std::fs::write(
            repo.join("openspec/changes/example/tasks.md"),
            "# Tasks\n\n## 1. Runtime\n<!-- specs: lifecycle/example -->\n\n- [x] 1.1 Done\n- [ ] 1.2 Pending\n",
        )
        .unwrap();

        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["visible work".into()]);

        let output = render_plan_list_text(&intent, repo);

        assert!(output.contains("Visible"), "{output}");
        assert!(output.contains("visible work"), "{output}");
        assert!(output.contains("OpenSpec"), "{output}");
        assert!(output.contains("example · active · 1/2"), "{output}");
        assert!(output.contains("1.1 Done"), "{output}");
    }

    #[test]
    fn render_plan_show_text_reports_stale_missing_plan() {
        let dir = tempfile::tempdir().unwrap();
        let intent = IntentDocument::default();

        let output = render_plan_show_text(&intent, dir.path(), "missing:plan");

        assert!(output.contains("Plan missing:plan"), "{output}");
        assert!(output.contains("stale"), "{output}");
        assert!(output.contains(STALE_PLAN_COPY), "{output}");
    }
}

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
