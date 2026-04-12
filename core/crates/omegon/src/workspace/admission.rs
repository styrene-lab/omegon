use super::runtime::heartbeat_is_stale;
use super::types::{
    AdmissionOutcome, Mutability, WorkspaceActionKind, WorkspaceAdmissionRequest, WorkspaceKind,
    WorkspaceLease, WorkspaceRole,
};

pub fn classify_admission(
    current: Option<&WorkspaceLease>,
    request: &WorkspaceAdmissionRequest,
    now_epoch_secs: i64,
    heartbeat_epoch_secs: Option<i64>,
) -> AdmissionOutcome {
    if let Some(reason) = authority_denial_reason(current, request) {
        return AdmissionOutcome::DeniedByAuthorityPolicy { reason };
    }

    match request.requested_mutability {
        Mutability::ReadOnly => AdmissionOutcome::GrantedReadOnly,
        Mutability::Mutable => match current {
            None => AdmissionOutcome::GrantedMutable,
            Some(lease) => {
                let stale = heartbeat_epoch_secs
                    .map(|heartbeat| heartbeat_is_stale(now_epoch_secs, heartbeat))
                    .unwrap_or(false);
                if stale {
                    return AdmissionOutcome::ConflictStaleLeaseAdoptable {
                        owner_session_id: lease.owner_session_id.clone(),
                    };
                }
                if lease.mutability == Mutability::ReadOnly {
                    AdmissionOutcome::GrantedMutable
                } else if request.requested_role == WorkspaceRole::CleaveChild {
                    AdmissionOutcome::ConflictCreateWorkspaceSuggested {
                        owner_session_id: lease.owner_session_id.clone(),
                    }
                } else {
                    AdmissionOutcome::ConflictReadOnlySuggested {
                        owner_session_id: lease.owner_session_id.clone(),
                    }
                }
            }
        },
    }
}

fn authority_denial_reason(
    current: Option<&WorkspaceLease>,
    request: &WorkspaceAdmissionRequest,
) -> Option<String> {
    match request.action {
        WorkspaceActionKind::ReleaseCut => {
            if request.requested_role != WorkspaceRole::Release {
                return Some("release cuts require a release workspace role".into());
            }
        }
        WorkspaceActionKind::BenchmarkRun => {
            if request.requested_role != WorkspaceRole::Benchmark {
                return Some(
                    "release-evaluation benchmark runs require a benchmark workspace role".into(),
                );
            }
        }
        _ => {}
    }

    if let Some(current) = current {
        if current.workspace_kind == WorkspaceKind::Vault
            && matches!(request.action, WorkspaceActionKind::ReleaseCut)
        {
            return Some("vault workspaces are not valid release-cut authority surfaces".into());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::workspace::types::{WorkspaceBackendKind, WorkspaceBindings, WorkspaceVcsRef};

    fn occupied_feature_lease() -> WorkspaceLease {
        WorkspaceLease {
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
            bindings: WorkspaceBindings::default(),
            branch: "feature/demo".into(),
            role: WorkspaceRole::Feature,
            workspace_kind: WorkspaceKind::Mixed,
            mutability: Mutability::Mutable,
            owner_session_id: Some("session-1".into()),
            owner_agent_id: Some("agent-1".into()),
            created_at: "2026-04-11T00:00:00Z".into(),
            last_heartbeat: "2026-04-11T00:00:10Z".into(),
            archived: false,
            archived_at: None,
            archive_reason: None,
            parent_workspace_id: None,
            source: "operator".into(),
        }
    }

    #[test]
    fn mutable_session_start_is_granted_when_workspace_is_free() {
        let req = WorkspaceAdmissionRequest {
            requested_role: WorkspaceRole::Feature,
            requested_kind: WorkspaceKind::Mixed,
            requested_mutability: Mutability::Mutable,
            session_id: Some("session-2".into()),
            action: WorkspaceActionKind::SessionStart,
        };
        assert_eq!(
            classify_admission(None, &req, 1_000, None),
            AdmissionOutcome::GrantedMutable
        );
    }

    #[test]
    fn read_only_attach_is_granted_even_when_workspace_is_occupied() {
        let req = WorkspaceAdmissionRequest {
            requested_role: WorkspaceRole::Feature,
            requested_kind: WorkspaceKind::Mixed,
            requested_mutability: Mutability::ReadOnly,
            session_id: Some("session-2".into()),
            action: WorkspaceActionKind::SessionStart,
        };
        assert_eq!(
            classify_admission(Some(&occupied_feature_lease()), &req, 1_000, Some(900)),
            AdmissionOutcome::GrantedReadOnly
        );
    }

    #[test]
    fn second_mutable_attach_suggests_read_only_for_normal_session() {
        let req = WorkspaceAdmissionRequest {
            requested_role: WorkspaceRole::Feature,
            requested_kind: WorkspaceKind::Mixed,
            requested_mutability: Mutability::Mutable,
            session_id: Some("session-2".into()),
            action: WorkspaceActionKind::SessionStart,
        };
        assert_eq!(
            classify_admission(Some(&occupied_feature_lease()), &req, 1_000, Some(900)),
            AdmissionOutcome::ConflictReadOnlySuggested {
                owner_session_id: Some("session-1".into())
            }
        );
    }

    #[test]
    fn stale_lease_is_adoptable() {
        let req = WorkspaceAdmissionRequest {
            requested_role: WorkspaceRole::Feature,
            requested_kind: WorkspaceKind::Mixed,
            requested_mutability: Mutability::Mutable,
            session_id: Some("session-2".into()),
            action: WorkspaceActionKind::SessionStart,
        };
        assert_eq!(
            classify_admission(Some(&occupied_feature_lease()), &req, 1_000, Some(600)),
            AdmissionOutcome::ConflictStaleLeaseAdoptable {
                owner_session_id: Some("session-1".into())
            }
        );
    }

    #[test]
    fn cleave_child_prefers_new_workspace_conflict_path() {
        let req = WorkspaceAdmissionRequest {
            requested_role: WorkspaceRole::CleaveChild,
            requested_kind: WorkspaceKind::Mixed,
            requested_mutability: Mutability::Mutable,
            session_id: Some("session-2".into()),
            action: WorkspaceActionKind::CleaveChildCreate,
        };
        assert_eq!(
            classify_admission(Some(&occupied_feature_lease()), &req, 1_000, Some(900)),
            AdmissionOutcome::ConflictCreateWorkspaceSuggested {
                owner_session_id: Some("session-1".into())
            }
        );
    }

    #[test]
    fn release_cut_requires_release_role() {
        let req = WorkspaceAdmissionRequest {
            requested_role: WorkspaceRole::Feature,
            requested_kind: WorkspaceKind::Mixed,
            requested_mutability: Mutability::Mutable,
            session_id: Some("session-2".into()),
            action: WorkspaceActionKind::ReleaseCut,
        };
        assert_eq!(
            classify_admission(None, &req, 1_000, None),
            AdmissionOutcome::DeniedByAuthorityPolicy {
                reason: "release cuts require a release workspace role".into()
            }
        );
    }

    #[test]
    fn benchmark_run_requires_benchmark_role() {
        let req = WorkspaceAdmissionRequest {
            requested_role: WorkspaceRole::Feature,
            requested_kind: WorkspaceKind::Mixed,
            requested_mutability: Mutability::Mutable,
            session_id: Some("session-2".into()),
            action: WorkspaceActionKind::BenchmarkRun,
        };
        assert_eq!(
            classify_admission(None, &req, 1_000, None),
            AdmissionOutcome::DeniedByAuthorityPolicy {
                reason: "release-evaluation benchmark runs require a benchmark workspace role"
                    .into()
            }
        );
    }
}
