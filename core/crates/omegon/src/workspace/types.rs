use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceRole {
    Primary,
    Feature,
    CleaveChild,
    Benchmark,
    Release,
    Exploratory,
    ReadOnly,
}

impl WorkspaceRole {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "primary" => Some(Self::Primary),
            "feature" => Some(Self::Feature),
            "cleave-child" => Some(Self::CleaveChild),
            "benchmark" => Some(Self::Benchmark),
            "release" => Some(Self::Release),
            "exploratory" => Some(Self::Exploratory),
            "read-only" => Some(Self::ReadOnly),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Feature => "feature",
            Self::CleaveChild => "cleave-child",
            Self::Benchmark => "benchmark",
            Self::Release => "release",
            Self::Exploratory => "exploratory",
            Self::ReadOnly => "read-only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceKind {
    Code,
    Vault,
    Knowledge,
    Spec,
    Mixed,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceBackendKind {
    LocalDir,
    GitWorktree,
    GitClone,
    JjCheckout,
    RemoteDir,
    PodVolume,
}

impl WorkspaceBackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalDir => "local-dir",
            Self::GitWorktree => "git-worktree",
            Self::GitClone => "git-clone",
            Self::JjCheckout => "jj-checkout",
            Self::RemoteDir => "remote-dir",
            Self::PodVolume => "pod-volume",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceVcsRef {
    pub vcs: String,
    pub branch: Option<String>,
    pub revision: Option<String>,
    pub remote: Option<String>,
}

impl WorkspaceKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "code" => Some(Self::Code),
            "vault" => Some(Self::Vault),
            "knowledge" => Some(Self::Knowledge),
            "spec" => Some(Self::Spec),
            "mixed" => Some(Self::Mixed),
            "generic" => Some(Self::Generic),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Vault => "vault",
            Self::Knowledge => "knowledge",
            Self::Spec => "spec",
            Self::Mixed => "mixed",
            Self::Generic => "generic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mutability {
    Mutable,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceActionKind {
    SessionStart,
    DaemonAttach,
    CleaveChildCreate,
    ReleaseCut,
    BenchmarkRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAdmissionRequest {
    pub requested_role: WorkspaceRole,
    pub requested_kind: WorkspaceKind,
    pub requested_mutability: Mutability,
    pub session_id: Option<String>,
    pub action: WorkspaceActionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionOutcome {
    GrantedMutable,
    GrantedReadOnly,
    ConflictReadOnlySuggested { owner_session_id: Option<String> },
    ConflictCreateWorkspaceSuggested { owner_session_id: Option<String> },
    ConflictStaleLeaseAdoptable { owner_session_id: Option<String> },
    DeniedByAuthorityPolicy { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceBindings {
    pub milestone_id: Option<String>,
    pub design_node_id: Option<String>,
    pub openspec_change: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceLease {
    pub project_id: String,
    pub workspace_id: String,
    pub label: String,
    pub path: String,
    pub backend_kind: WorkspaceBackendKind,
    pub vcs_ref: Option<WorkspaceVcsRef>,
    pub bindings: WorkspaceBindings,
    pub branch: String,
    pub role: WorkspaceRole,
    pub workspace_kind: WorkspaceKind,
    pub mutability: Mutability,
    pub owner_session_id: Option<String>,
    pub owner_agent_id: Option<String>,
    pub created_at: String,
    pub last_heartbeat: String,
    pub parent_workspace_id: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub workspace_id: String,
    pub label: String,
    pub path: String,
    pub backend_kind: WorkspaceBackendKind,
    pub vcs_ref: Option<WorkspaceVcsRef>,
    pub bindings: WorkspaceBindings,
    pub branch: String,
    pub role: WorkspaceRole,
    pub workspace_kind: WorkspaceKind,
    pub mutability: Mutability,
    pub owner_session_id: Option<String>,
    pub last_heartbeat: String,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRegistry {
    pub project_id: String,
    pub repo_root: String,
    pub workspaces: Vec<WorkspaceSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_admission_request_holds_role_kind_and_action() {
        let req = WorkspaceAdmissionRequest {
            requested_role: WorkspaceRole::Feature,
            requested_kind: WorkspaceKind::Mixed,
            requested_mutability: Mutability::Mutable,
            session_id: Some("session-1".into()),
            action: WorkspaceActionKind::SessionStart,
        };
        assert_eq!(req.requested_role, WorkspaceRole::Feature);
        assert_eq!(req.requested_kind, WorkspaceKind::Mixed);
        assert_eq!(req.requested_mutability, Mutability::Mutable);
        assert_eq!(req.action, WorkspaceActionKind::SessionStart);
    }

    #[test]
    fn workspace_lease_round_trip() {
        let lease = WorkspaceLease {
            project_id: "proj".into(),
            workspace_id: "ws".into(),
            label: "feature-demo".into(),
            path: "/tmp/ws".into(),
            backend_kind: WorkspaceBackendKind::GitWorktree,
            vcs_ref: Some(WorkspaceVcsRef {
                vcs: "git".into(),
                branch: Some("feature/demo".into()),
                revision: None,
                remote: Some("origin".into()),
            }),
            bindings: WorkspaceBindings {
                milestone_id: Some("0.15.10".into()),
                design_node_id: Some("workspace-ownership-model".into()),
                openspec_change: None,
            },
            branch: "feature/demo".into(),
            role: WorkspaceRole::Feature,
            workspace_kind: WorkspaceKind::Mixed,
            mutability: Mutability::Mutable,
            owner_session_id: Some("session-1".into()),
            owner_agent_id: Some("agent-1".into()),
            created_at: "2026-04-11T00:00:00Z".into(),
            last_heartbeat: "2026-04-11T00:00:10Z".into(),
            parent_workspace_id: Some("parent".into()),
            source: "operator".into(),
        };
        let json = serde_json::to_string(&lease).unwrap();
        let decoded: WorkspaceLease = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, lease);
    }

    #[test]
    fn workspace_registry_round_trip() {
        let registry = WorkspaceRegistry {
            project_id: "proj".into(),
            repo_root: "/repo".into(),
            workspaces: vec![WorkspaceSummary {
                workspace_id: "ws".into(),
                label: "primary".into(),
                path: "/repo".into(),
                backend_kind: WorkspaceBackendKind::LocalDir,
                vcs_ref: Some(WorkspaceVcsRef {
                    vcs: "git".into(),
                    branch: Some("main".into()),
                    revision: None,
                    remote: Some("origin".into()),
                }),
                bindings: WorkspaceBindings {
                    milestone_id: Some("0.15.10".into()),
                    design_node_id: Some("release-candidate-system".into()),
                    openspec_change: None,
                },
                branch: "main".into(),
                role: WorkspaceRole::Primary,
                workspace_kind: WorkspaceKind::Code,
                mutability: Mutability::Mutable,
                owner_session_id: None,
                last_heartbeat: "2026-04-11T00:00:10Z".into(),
                stale: false,
            }],
        };
        let json = serde_json::to_string(&registry).unwrap();
        let decoded: WorkspaceRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, registry);
    }
}
