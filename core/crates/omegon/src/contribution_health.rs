//! Latest guarded discovery outcome, independent of renderer and active inventory.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionKind {
    Skills,
    Plugins,
    Extensions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LoadingState {
    Absent,
    Loaded { count: usize },
    Blocked { code: String, causes: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeHealth {
    pub kind: ContributionKind,
    pub scope: String,
    pub root: PathBuf,
    #[serde(flatten)]
    pub outcome: LoadingState,
}

/// Keep diagnostics bounded while retaining the outer context and deepest cause.
fn bounded_causes(error: &anyhow::Error) -> Vec<String> {
    const MAX_CAUSES: usize = 8;
    const MAX_CAUSE_BYTES: usize = 2048;
    fn bounded(mut message: String) -> String {
        if message.len() > MAX_CAUSE_BYTES {
            let mut end = MAX_CAUSE_BYTES - "… [truncated]".len();
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
            message.push_str("… [truncated]");
        }
        message
    }
    let mut causes = Vec::new();
    for cause in error.chain() {
        let message = bounded(cause.to_string());
        if causes.len() < MAX_CAUSES {
            causes.push(message);
        } else {
            causes[MAX_CAUSES - 1] = message;
        }
    }
    causes
}

impl ScopeHealth {
    pub fn new(kind: ContributionKind, scope: &str, root: &Path, outcome: LoadingState) -> Self {
        Self {
            kind,
            scope: scope.into(),
            root: root.into(),
            outcome,
        }
    }
    pub fn blocked(
        kind: ContributionKind,
        scope: &str,
        root: &Path,
        error: &anyhow::Error,
    ) -> Self {
        use omegon_maintenance_contracts::ContractError;
        let code = match error.downcast_ref::<ContractError>() {
            Some(ContractError::HomeIdentityMismatch { .. }) => "home_identity_mismatch",
            Some(ContractError::HomeRecoveryPending) => "home_recovery_pending",
            Some(
                ContractError::InvalidJson(_)
                | ContractError::DuplicateKey(_)
                | ContractError::RecordTooLarge
                | ContractError::UnsupportedSchema(_)
                | ContractError::RecordKind { .. },
            ) => "invalid_maintenance_record",
            Some(_) => "maintenance_admission_failed",
            None if error.downcast_ref::<std::io::Error>().is_some() => "filesystem_error",
            None => "contribution_load_failed",
        };
        Self::new(
            kind,
            scope,
            root,
            LoadingState::Blocked {
                code: code.into(),
                causes: bounded_causes(error),
            },
        )
    }
    pub fn is_blocked(&self) -> bool {
        matches!(self.outcome, LoadingState::Blocked { .. })
    }
    pub fn summary(&self) -> String {
        let kind = match self.kind {
            ContributionKind::Skills => "skills",
            ContributionKind::Plugins => "plugins",
            ContributionKind::Extensions => "extensions",
        };
        match &self.outcome {
            LoadingState::Absent => {
                format!("{kind} ({}) absent — {}", self.scope, self.root.display())
            }
            LoadingState::Loaded { count } => format!(
                "{kind} ({}) loaded: {count} — {}",
                self.scope,
                self.root.display()
            ),
            LoadingState::Blocked { code, causes } => format!(
                "{kind} ({}) blocked [{code}] — {}: {}",
                self.scope,
                self.root.display(),
                causes.join(": ")
            ),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ContributionHealth(Arc<Mutex<BTreeMap<(ContributionKind, String), ScopeHealth>>>);
impl ContributionHealth {
    pub(crate) fn replace_kind(&self, kind: ContributionKind, scopes: Vec<ScopeHealth>) {
        let mut current = self.0.lock().unwrap_or_else(|e| e.into_inner());
        current.retain(|(existing, _), _| *existing != kind);
        for scope in scopes {
            current.insert((scope.kind, scope.scope.clone()), scope);
        }
    }
    pub(crate) fn replace_scope(&self, scope: ScopeHealth) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert((scope.kind, scope.scope.clone()), scope);
    }
    pub(crate) fn snapshot(&self) -> Vec<ScopeHealth> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }
}

/// No repeat for identical failures; successful reload removes the current warning.
pub(crate) fn change_notice(previous: &[ScopeHealth], current: &[ScopeHealth]) -> Option<String> {
    let before: Vec<_> = previous.iter().filter(|scope| scope.is_blocked()).collect();
    let after: Vec<_> = current.iter().filter(|scope| scope.is_blocked()).collect();
    if before == after {
        return None;
    }
    if after.is_empty() {
        return Some("Contribution loading recovered. /status for details.".into());
    }
    Some(format!(
        "{} contribution scope{} could not load. /status for details.",
        after.len(),
        if after.len() == 1 { "" } else { "s" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contribution_health_error_payload_is_bounded_and_retains_root_cause() {
        let mut error = anyhow::anyhow!("root failure");
        for _ in 0..20 {
            error = error.context("界".repeat(3000));
        }
        let causes = bounded_causes(&error);
        assert_eq!(causes.len(), 8);
        assert!(causes.iter().all(|cause| cause.len() <= 2048));
        assert!(causes[0].ends_with("[truncated]"));
        assert_eq!(causes.last().unwrap(), "root failure");
    }

    #[test]
    fn contribution_health_identity_mismatch_retains_typed_cause_in_shared_status() {
        let observed =
            omegon_maintenance_contracts::PathIdentityV1::unix(b"/home/operator/.omegon", 2, 3)
                .unwrap();
        let mut stored = observed.clone();
        stored.device = 1;
        let error = anyhow::Error::new(
            omegon_maintenance_contracts::ContractError::HomeIdentityMismatch {
                stored: Box::new(stored),
                observed: Box::new(observed),
            },
        )
        .context("opening installed skills");
        let scope = ScopeHealth::blocked(
            ContributionKind::Skills,
            "user",
            Path::new("/home/operator/.omegon/skills"),
            &error,
        );
        let bus = crate::bus::EventBus::new();
        bus.contribution_health()
            .replace_kind(ContributionKind::Skills, vec![scope.clone()]);
        let mut status = crate::status::HarnessStatus::default();
        status.update_from_bus(&bus);
        assert_eq!(status.contribution_loading, vec![scope]);
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(
            json["contribution_loading"][0]["code"],
            "home_identity_mismatch"
        );
        let rendered = crate::bootstrap_projection::render_bootstrap(&status, false);
        assert!(rendered.contains("home_identity_mismatch"));
        assert!(rendered.contains("/home/operator/.omegon/skills"));
        assert!(rendered.contains("opening installed skills"));
        assert!(rendered.contains("different home identity"));
    }

    #[test]
    fn contribution_health_replacement_preserves_other_scopes_and_clears_failure() {
        let health = ContributionHealth::default();
        let error = anyhow::anyhow!("broken metadata").context("opening guarded scope");
        let failed = ScopeHealth::blocked(
            ContributionKind::Skills,
            "user",
            Path::new("/skills"),
            &error,
        );
        let absent = ScopeHealth::new(
            ContributionKind::Plugins,
            "project",
            Path::new("/plugins"),
            LoadingState::Absent,
        );
        health.replace_kind(ContributionKind::Skills, vec![failed.clone()]);
        health.replace_kind(ContributionKind::Plugins, vec![absent.clone()]);
        assert_eq!(health.snapshot(), vec![failed.clone(), absent.clone()]);
        assert!(change_notice(&[], &health.snapshot()).is_some());
        assert_eq!(change_notice(&health.snapshot(), &health.snapshot()), None);
        let before = health.snapshot();
        let healthy = ScopeHealth::new(
            ContributionKind::Skills,
            "user",
            Path::new("/skills"),
            LoadingState::Loaded { count: 0 },
        );
        health.replace_kind(ContributionKind::Skills, vec![healthy.clone()]);
        assert_eq!(health.snapshot(), vec![healthy, absent]);
        assert!(
            change_notice(&before, &health.snapshot())
                .unwrap()
                .contains("recovered")
        );
        let LoadingState::Blocked { causes, .. } = failed.outcome else {
            panic!()
        };
        assert_eq!(causes, ["opening guarded scope", "broken metadata"]);
    }
}
