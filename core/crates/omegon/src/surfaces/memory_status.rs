//! Read-only memory/federation status projection.
//!
//! Durable-memory state comes from the latest managed-service snapshot. This
//! projection never probes the live store or its synchronization files.

use std::path::{Path, PathBuf};

/// Observed state, not a provider probe. `Configured` deliberately does not claim
/// credentials, model identity, or a successful inference request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    #[default]
    Unknown,
    Disabled,
    Configured,
    Ready,
    Unavailable,
    Degraded,
}

/// Content-free reasons suitable for every status consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityReason {
    OperatorDisabled,
    ChildSession,
    InvalidConfiguration,
    StorageUnavailable,
    ProviderUnavailable,
    DeadlineExceeded,
    Cancelled,
    IndexIncomplete,
    IndexUnobserved,
    ProviderUnverified,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComponentReadiness {
    pub state: CapabilityState,
    pub reason: Option<CapabilityReason>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemoryCapabilityReadiness {
    pub storage: ComponentReadiness,
    pub extraction: ComponentReadiness,
    pub embeddings: ComponentReadiness,
    pub keyword_retrieval: ComponentReadiness,
    pub semantic_retrieval: ComponentReadiness,
    /// None means the owner has not supplied an index observation, not zero work.
    pub pending_indexing: Option<usize>,
    #[serde(default)]
    pub indexing: Option<omegon_memory::EmbeddingIndexingSummary>,
}

/// Pure projection: callers supply observations from normal managed operations.
/// There are intentionally no service handles, paths, or inference callbacks here.
pub fn project_memory_capabilities(
    storage_available: bool,
    extraction: ComponentReadiness,
    embeddings: ComponentReadiness,
    pending_indexing: Option<usize>,
) -> MemoryCapabilityReadiness {
    let storage = if storage_available {
        ComponentReadiness {
            state: CapabilityState::Ready,
            reason: None,
        }
    } else {
        ComponentReadiness {
            state: CapabilityState::Unavailable,
            reason: Some(CapabilityReason::StorageUnavailable),
        }
    };
    let semantic_retrieval = if !storage_available {
        storage
    } else if pending_indexing.is_some_and(|pending| pending > 0) {
        ComponentReadiness {
            state: CapabilityState::Degraded,
            reason: Some(CapabilityReason::IndexIncomplete),
        }
    } else if embeddings.state == CapabilityState::Ready && pending_indexing == Some(0) {
        embeddings
    } else {
        ComponentReadiness {
            state: CapabilityState::Degraded,
            reason: embeddings
                .reason
                .or(Some(if embeddings.state == CapabilityState::Ready {
                    CapabilityReason::IndexUnobserved
                } else {
                    CapabilityReason::ProviderUnverified
                })),
        }
    };
    MemoryCapabilityReadiness {
        storage,
        extraction,
        embeddings,
        keyword_retrieval: storage,
        semantic_retrieval,
        pending_indexing,
        indexing: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationMode {
    OneOff,
    OrdinaryGit,
    LifecycleProject,
    Federation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryAuthority {
    GitJsonl { paths: Vec<PathBuf> },
    LocalIndexOnly,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryIndexState {
    Fresh,
    Stale,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSummary {
    pub root: PathBuf,
    pub branch: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFederationStatusProjection {
    pub cwd: PathBuf,
    pub mode: CoordinationMode,
    pub signals: Vec<String>,
    pub git: Option<GitSummary>,
    pub memory_authority: MemoryAuthority,
    pub memory_index: MemoryIndexState,
    pub recommended_behavior: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFederationObservation {
    pub cwd: PathBuf,
    pub git: Option<GitSummary>,
    pub lifecycle_signals: Vec<String>,
    pub federation_signals: Vec<String>,
    pub memory_authority: MemoryAuthority,
    pub memory_index: MemoryIndexState,
}

impl MemoryFederationStatusProjection {
    pub fn git_root_or_cwd(&self) -> &Path {
        self.git
            .as_ref()
            .map(|summary| summary.root.as_path())
            .unwrap_or(self.cwd.as_path())
    }
}

pub fn project_memory_federation_status(
    observation: MemoryFederationObservation,
) -> MemoryFederationStatusProjection {
    let MemoryFederationObservation {
        cwd,
        git,
        lifecycle_signals,
        federation_signals,
        memory_authority,
        memory_index,
    } = observation;
    let mut signals = Vec::new();

    if git.is_some() {
        signals.push("git".to_string());
    }

    signals.extend(lifecycle_signals.iter().cloned());
    signals.extend(federation_signals.iter().cloned());
    if !matches!(memory_authority, MemoryAuthority::None) {
        signals.push("memory:managed".to_string());
    }
    let mode = if !federation_signals.is_empty() {
        CoordinationMode::Federation
    } else if !lifecycle_signals.is_empty() {
        CoordinationMode::LifecycleProject
    } else if git.is_some() {
        CoordinationMode::OrdinaryGit
    } else {
        CoordinationMode::OneOff
    };

    let recommended_behavior = recommendation(mode, &memory_authority, memory_index).to_string();

    MemoryFederationStatusProjection {
        cwd,
        mode,
        signals,
        git,
        memory_authority,
        memory_index,
        recommended_behavior,
    }
}

fn recommendation(
    mode: CoordinationMode,
    authority: &MemoryAuthority,
    index: MemoryIndexState,
) -> &'static str {
    match (mode, authority, index) {
        (CoordinationMode::OneOff, MemoryAuthority::None, _) => {
            "No Git-tracked memory authority detected; treat memory as local/session scoped."
        }
        (_, MemoryAuthority::GitJsonl { .. }, MemoryIndexState::Stale) => {
            "Git-tracked JSONL facts are authoritative; rebuild the local memory index, then use normal Git fetch/merge/rebase for checkout continuity."
        }
        (_, MemoryAuthority::GitJsonl { .. }, _) => {
            "Git-tracked JSONL facts are authoritative; use normal Git fetch/merge/rebase for checkout continuity."
        }
        (_, MemoryAuthority::LocalIndexOnly, _) => {
            "Only a local memory index was detected; do not treat it as cross-checkout coordination state."
        }
        _ => "No project memory facts detected; no memory synchronization action is applicable.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave5_capability_matrix_keeps_optional_components_independent() {
        for storage in [false, true] {
            for extraction_ready in [false, true] {
                for embeddings_ready in [false, true] {
                    let component = |ready| ComponentReadiness {
                        state: if ready {
                            CapabilityState::Ready
                        } else {
                            CapabilityState::Unavailable
                        },
                        reason: (!ready).then_some(CapabilityReason::ProviderUnavailable),
                    };
                    let extraction = component(extraction_ready);
                    let embeddings = component(embeddings_ready);
                    let result =
                        project_memory_capabilities(storage, extraction, embeddings, Some(0));
                    assert_eq!(result.extraction, extraction);
                    assert_eq!(result.embeddings, embeddings);
                    assert_eq!(
                        result.keyword_retrieval.state == CapabilityState::Ready,
                        storage
                    );
                    assert_eq!(
                        result.semantic_retrieval.state == CapabilityState::Ready,
                        storage && embeddings_ready
                    );
                }
            }
        }
        let embeddings_only = project_memory_capabilities(
            true,
            ComponentReadiness {
                state: CapabilityState::Disabled,
                reason: Some(CapabilityReason::OperatorDisabled),
            },
            ComponentReadiness {
                state: CapabilityState::Ready,
                reason: None,
            },
            Some(0),
        );
        assert_eq!(embeddings_only.extraction.state, CapabilityState::Disabled);
        assert_eq!(
            embeddings_only.semantic_retrieval.state,
            CapabilityState::Ready
        );
    }

    #[test]
    fn wave5_outage_and_pending_repair_are_independent_read_only_observations() {
        let extraction = ComponentReadiness {
            state: CapabilityState::Unavailable,
            reason: Some(CapabilityReason::ProviderUnavailable),
        };
        let embeddings = ComponentReadiness {
            state: CapabilityState::Degraded,
            reason: Some(CapabilityReason::DeadlineExceeded),
        };
        let before = project_memory_capabilities(true, extraction, embeddings, Some(3));
        let encoded = serde_json::to_value(&before).unwrap();
        for _ in 0..3 {
            assert_eq!(
                serde_json::to_value(project_memory_capabilities(
                    true,
                    extraction,
                    embeddings,
                    Some(3)
                ))
                .unwrap(),
                encoded
            );
        }
        assert_eq!(
            before.extraction.reason,
            Some(CapabilityReason::ProviderUnavailable)
        );
        assert_eq!(
            before.embeddings.reason,
            Some(CapabilityReason::DeadlineExceeded)
        );
        assert_eq!(
            before.semantic_retrieval.reason,
            Some(CapabilityReason::IndexIncomplete)
        );
    }

    #[test]
    fn wave5_configuration_and_unknown_index_do_not_claim_semantic_readiness() {
        let configured = ComponentReadiness {
            state: CapabilityState::Configured,
            reason: None,
        };
        let result = project_memory_capabilities(true, configured, configured, None);
        assert_eq!(result.pending_indexing, None);
        assert_eq!(result.extraction.state, CapabilityState::Configured);
        assert_eq!(result.semantic_retrieval.state, CapabilityState::Degraded);
    }
    fn observation(
        git: bool,
        lifecycle: bool,
        authority: MemoryAuthority,
    ) -> MemoryFederationObservation {
        MemoryFederationObservation {
            cwd: PathBuf::from("/workspace"),
            git: git.then(|| GitSummary {
                root: PathBuf::from("/workspace"),
                branch: Some("main".into()),
                dirty: false,
            }),
            lifecycle_signals: lifecycle.then(|| "AGENTS.md".into()).into_iter().collect(),
            federation_signals: Vec::new(),
            memory_authority: authority,
            memory_index: MemoryIndexState::Missing,
        }
    }

    #[test]
    fn non_git_directory_is_one_off_without_memory_authority() {
        let projection =
            project_memory_federation_status(observation(false, false, MemoryAuthority::None));

        assert_eq!(projection.mode, CoordinationMode::OneOff);
        assert_eq!(projection.memory_authority, MemoryAuthority::None);
        assert!(projection.recommended_behavior.contains("local/session"));
    }

    #[test]
    fn git_repo_without_lifecycle_signals_is_ordinary_git() {
        let projection =
            project_memory_federation_status(observation(true, false, MemoryAuthority::None));

        assert_eq!(projection.mode, CoordinationMode::OrdinaryGit);
        assert!(projection.signals.contains(&"git".to_string()));
    }

    #[test]
    fn owner_observation_controls_live_memory_authority() {
        let projection =
            project_memory_federation_status(observation(true, true, MemoryAuthority::None));

        assert_eq!(projection.mode, CoordinationMode::LifecycleProject);
        assert_eq!(projection.memory_authority, MemoryAuthority::None);
        assert_eq!(projection.memory_index, MemoryIndexState::Missing);
        assert!(!projection.signals.contains(&"memory:git-jsonl".into()));
    }

    #[test]
    fn managed_observation_projects_local_index_state() {
        let projection = project_memory_federation_status(observation(
            true,
            false,
            MemoryAuthority::LocalIndexOnly,
        ));

        assert_eq!(projection.memory_authority, MemoryAuthority::LocalIndexOnly);
        assert_eq!(projection.memory_index, MemoryIndexState::Missing);
    }
}
