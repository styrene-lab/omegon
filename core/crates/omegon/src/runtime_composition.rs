//! Pure decisions used while composing the interactive runtime.
//!
//! This module deliberately owns no I/O or process-global state. Keeping these
//! transformations separate from `main.rs` makes startup wiring testable without
//! constructing providers, terminals, or an agent loop.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CompositionInspectionV1 {
    pub(crate) schema_version: u32,
    pub(crate) artifact_profile: String,
    pub(crate) canonical_entrypoint: Vec<String>,
    pub(crate) profile: String,
    pub(crate) runtime_mode: String,
    pub(crate) surfaces: Vec<String>,
    pub(crate) absent_optional: Vec<String>,
    pub(crate) startup_tasks: CountedOwnersV1,
    pub(crate) model_schema: CountedOwnersV1,
    pub(crate) resident_capabilities: Vec<String>,
    pub(crate) callable_capabilities: Vec<String>,
    pub(crate) external_processes: Vec<ExternalProcessV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) functional_probe: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CountedOwnersV1 {
    pub(crate) count: usize,
    pub(crate) owners: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ExternalProcessV1 {
    pub(crate) owner: String,
    pub(crate) state: String,
    pub(crate) pid: Option<u32>,
}

pub(crate) fn inspect_runtime_composition(
    profile: &str,
    bus: &crate::bus::EventBus,
) -> anyhow::Result<CompositionInspectionV1> {
    let (runtime_mode, surfaces, absent_optional) = validated_profile_state(profile)?;
    let definitions = bus.callable_tool_definitions_by_owner();
    let callable_capabilities = definitions
        .iter()
        .map(|(_, definition)| format!("tool:{}", definition.name))
        .collect::<Vec<_>>();
    let mut schema_owners = BTreeMap::new();
    for (owner, definition) in &definitions {
        let tokens =
            crate::loop_context::estimate_tool_schema_tokens(std::slice::from_ref(definition));
        *schema_owners.entry(owner.clone()).or_default() += tokens;
    }
    let mut startup_owners = bus.active_startup_resource_owners();
    if profile != "kernel" {
        startup_owners.insert("system:inference-discovery".to_string(), 1);
    }

    Ok(CompositionInspectionV1 {
        schema_version: 1,
        artifact_profile: compiled_artifact_profile().to_string(),
        canonical_entrypoint: canonical_entrypoint()
            .iter()
            .map(|part| (*part).to_string())
            .collect(),
        profile: profile.to_string(),
        runtime_mode: runtime_mode.to_string(),
        surfaces: surfaces.into_iter().map(str::to_string).collect(),
        absent_optional: absent_optional.into_iter().map(str::to_string).collect(),
        startup_tasks: CountedOwnersV1 {
            count: startup_owners.values().sum(),
            owners: startup_owners,
        },
        model_schema: CountedOwnersV1 {
            count: schema_owners.values().sum(),
            owners: schema_owners,
        },
        resident_capabilities: resident_capabilities(profile),
        callable_capabilities,
        external_processes: Vec::new(),
        functional_probe: None,
    })
}

pub(crate) fn external_process_inventory(
    health: Vec<crate::extensions::ExtensionProcessHealth>,
) -> Vec<ExternalProcessV1> {
    let mut processes = health
        .into_iter()
        .map(|process| ExternalProcessV1 {
            owner: process.name,
            state: match process.state {
                crate::extensions::ExtensionProcessState::Healthy => "healthy",
                crate::extensions::ExtensionProcessState::Unavailable => "unavailable",
                crate::extensions::ExtensionProcessState::Replacing => "replacing",
                crate::extensions::ExtensionProcessState::ShuttingDown => "shutting_down",
            }
            .to_string(),
            pid: process.pid,
        })
        .collect::<Vec<_>>();
    processes.sort_by(|left, right| left.owner.cmp(&right.owner));
    processes
}

pub(crate) async fn execute_composition_probe(
    probe: &str,
    cwd: &std::path::Path,
    bus: &crate::bus::EventBus,
) -> anyhow::Result<serde_json::Value> {
    let cancellation = tokio_util::sync::CancellationToken::new();
    match probe {
        "core-read" => {
            let fixture = cwd.join("composition-probe.txt");
            let result = bus
                .execute_tool(
                    "read",
                    "composition-core-read",
                    serde_json::json!({"path": fixture}),
                    cancellation.clone(),
                )
                .await?;
            let rendered = serde_json::to_string(&result.content)?;
            if !rendered.contains("omegon-composition-core-probe") {
                anyhow::bail!("kernel core-read probe did not return the fixture marker");
            }
            let absence = bus
                .execute_tool(
                    "codebase_search",
                    "composition-codescan-absence",
                    serde_json::json!({"query": "omegon-composition-core-probe"}),
                    cancellation,
                )
                .await?;
            if absence
                .details
                .get("code")
                .and_then(serde_json::Value::as_str)
                != Some("service:unavailable")
            {
                anyhow::bail!("kernel-only codescan contract was not typed unavailable");
            }
            Ok(serde_json::json!({
                "name": probe,
                "status": "ok",
                "codescan": "service:unavailable"
            }))
        }
        "codescan-search" => {
            let indexed = bus
                .execute_tool(
                    "codebase_index",
                    "composition-codescan-index",
                    serde_json::json!({"invalidate": true}),
                    cancellation.clone(),
                )
                .await?;
            if indexed.details["code"] == "service:disabled" {
                let direct = bus
                    .execute_tool(
                        "codebase_search",
                        "composition-codescan-disabled",
                        serde_json::json!({"query": "omegon_composition_codescan_probe"}),
                        cancellation,
                    )
                    .await?;
                if direct.details["code"] != "service:disabled" {
                    anyhow::bail!("codescan direct invocation did not preserve disabled state");
                }
                return Ok(serde_json::json!({
                    "name": probe,
                    "status": "disabled",
                    "code": direct.details["code"],
                    "component_id": direct.details["component_id"],
                    "determining_policy_source": direct.details["determining_policy_source"],
                }));
            }
            if indexed.details["code"] == "service:unavailable" {
                return Ok(serde_json::json!({
                    "name": probe,
                    "status": "unavailable",
                    "code": "service:unavailable",
                    "component_id": "core:codescan",
                }));
            }
            let result = bus
                .execute_tool(
                    "codebase_search",
                    "composition-codescan-search",
                    serde_json::json!({
                        "query": "omegon_composition_codescan_probe",
                        "scope": "code",
                        "max_results": 5
                    }),
                    cancellation,
                )
                .await?;
            if !result.details["results"]
                .as_array()
                .is_some_and(|results| !results.is_empty())
            {
                anyhow::bail!("additive codescan probe did not restore search");
            }
            Ok(serde_json::json!({
                "name": probe,
                "status": "ok",
                "component_id": "core:codescan",
                "wire_manifest_id": crate::codescan_service::CODESCAN_EXTENSION,
                "service_id": omegon_codescan_contracts::CODESCAN_SERVICE_ID,
                "protocol_version": omegon_codescan_contracts::CODESCAN_PROTOCOL_VERSION,
                "service_provenance": result.details["service_provenance"]
            }))
        }
        "product-inspect" => Ok(serde_json::json!({"name": probe, "status": "ok"})),
        _ => anyhow::bail!("unknown composition probe: {probe}"),
    }
}

fn resident_capabilities(profile: &str) -> Vec<String> {
    if profile == "kernel" {
        return [
            "system:constitutional-kernel",
            "system:default-loop",
            "system:host-effects",
            "feature:codescan-adapter",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
    }
    omegon_maintenance_contracts::OMEGON_REQUIRED_RESIDENT_IDENTITIES
        .iter()
        .chain(omegon_maintenance_contracts::OMEGON_OPTIONAL_RESIDENT_IDENTITIES)
        .map(|identity| (*identity).to_string())
        .collect()
}

pub(crate) const fn compiled_artifact_profile() -> &'static str {
    if cfg!(feature = "kernel-host") {
        "kernel-host-v1"
    } else if cfg!(feature = "task-capsule") {
        "task-capsule-v0"
    } else if cfg!(all(
        feature = "tui",
        feature = "self-update",
        feature = "local-embeddings"
    )) {
        "full-product-local-embeddings"
    } else if cfg!(all(feature = "tui", feature = "self-update")) {
        "full-product"
    } else if cfg!(all(not(feature = "tui"), not(feature = "self-update"))) {
        "shrinking-host"
    } else {
        "custom-host"
    }
}

fn canonical_entrypoint() -> &'static [&'static str] {
    if cfg!(feature = "task-capsule") {
        &["omegon", "run"]
    } else {
        &["omegon"]
    }
}

pub(crate) fn validated_runtime_mode(profile: &str) -> anyhow::Result<&'static str> {
    Ok(validated_profile_state(profile)?.0)
}

fn validated_profile_state(
    profile: &str,
) -> anyhow::Result<(&'static str, Vec<&'static str>, Vec<&'static str>)> {
    if cfg!(feature = "kernel-host") {
        if profile != "kernel" {
            anyhow::bail!("composition profile '{profile}' is incompatible with kernel-host-v1");
        }
    } else if cfg!(feature = "task-capsule") {
        if profile != "task-capsule" {
            anyhow::bail!("composition profile '{profile}' is incompatible with task-capsule-v0");
        }
    } else if matches!(profile, "kernel" | "task-capsule") {
        anyhow::bail!("{profile} composition requires its matching artifact feature");
    }
    if !cfg!(feature = "tui") && matches!(profile, "interactive" | "full") {
        anyhow::bail!("composition profile '{profile}' requires the tui feature");
    }
    profile_state(profile)
}

fn profile_state(
    profile: &str,
) -> anyhow::Result<(&'static str, Vec<&'static str>, Vec<&'static str>)> {
    match profile {
        "kernel" => Ok((
            "kernel",
            vec!["agent-loop", "bounded-task"],
            vec![
                "context-compaction",
                "git",
                "lifecycle",
                "memory",
                "provider-clients",
                "shipped-content",
                "tui",
                "self-update",
            ],
        )),
        "task-capsule" => Ok((
            "bounded-task",
            vec!["agent-loop", "bounded-task"],
            vec!["tui", "self-update"],
        )),
        "interactive" => Ok(("tui", vec!["agent-loop", "tui"], vec![])),
        "headless" => Ok(("headless", vec!["agent-loop", "bounded-task"], vec!["tui"])),
        "daemon" => Ok(("daemon", vec!["agent-loop", "control-plane"], vec!["tui"])),
        "full" => Ok((
            "full",
            vec!["agent-loop", "bounded-task", "control-plane", "tui"],
            vec![],
        )),
        _ => anyhow::bail!("unknown composition profile: {profile}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InteractiveStartupModelDecision {
    pub(crate) selected_model: String,
    pub(crate) bridge_model: String,
    pub(crate) provider_connected: bool,
    pub(crate) use_null_bridge: bool,
}

/// Only an actual selected route may request startup credential recovery.
/// Empty preview surfaces must not infer a provider from their placeholder.
pub(crate) fn startup_credential_refresh_provider<'a>(
    requested_model: &str,
    route: &'a crate::route::ProviderRoute,
) -> Option<&'a str> {
    if requested_model.trim().is_empty() {
        return None;
    }
    match route {
        crate::route::ProviderRoute::Disconnected {
            reason:
                crate::route::DisconnectedReason::MissingCredentials { provider, .. }
                | crate::route::DisconnectedReason::ExpiredCredentials { provider, .. },
            ..
        } if !provider.is_empty() => Some(provider.as_str()),
        _ => None,
    }
}

pub(crate) fn decide_interactive_startup_model(
    selected_model: &str,
    resolved_model: &str,
    resolved_available: bool,
) -> InteractiveStartupModelDecision {
    InteractiveStartupModelDecision {
        selected_model: selected_model.to_string(),
        bridge_model: resolved_model.to_string(),
        provider_connected: resolved_available,
        use_null_bridge: !resolved_available,
    }
}

pub(crate) fn restart_args_for_session(
    args: impl IntoIterator<Item = String>,
    session_id: &str,
) -> Vec<String> {
    let mut filtered = Vec::new();
    let mut skip_resume_value = false;
    for arg in args {
        if skip_resume_value {
            skip_resume_value = false;
            continue;
        }
        if arg == "--fresh" {
            continue;
        }
        if arg == "--resume" {
            skip_resume_value = true;
            continue;
        }
        if arg.starts_with("--resume=") {
            continue;
        }
        filtered.push(arg);
    }
    filtered.push("--resume".to_string());
    filtered.push(session_id.to_string());
    filtered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_args_replace_all_session_selection_forms() {
        let args = vec![
            "omegon".into(),
            "--fresh".into(),
            "--resume=old-a".into(),
            "--resume".into(),
            "old-b".into(),
            "--model".into(),
            "test-model".into(),
        ];

        assert_eq!(
            restart_args_for_session(args, "new-session"),
            vec!["omegon", "--model", "test-model", "--resume", "new-session"]
        );
    }

    #[test]
    fn restart_args_preserve_trailing_resume_flag_without_dropping_unrelated_args() {
        assert_eq!(
            restart_args_for_session(
                vec![
                    "omegon".into(),
                    "--model".into(),
                    "test-model".into(),
                    "--resume".into()
                ],
                "new-session",
            ),
            vec!["omegon", "--model", "test-model", "--resume", "new-session"]
        );
    }

    #[test]
    fn recovery_auth_startup_refresh_requires_requested_route() {
        let route = crate::route::ProviderRoute::Disconnected {
            selected: crate::route::ModelRouteSpec::parse("anthropic:fixture"),
            reason: crate::route::DisconnectedReason::ExpiredCredentials {
                provider: "anthropic".into(),
                refreshable: true,
            },
        };
        assert_eq!(startup_credential_refresh_provider("", &route), None);
        assert_eq!(startup_credential_refresh_provider("   ", &route), None);
        assert_eq!(
            startup_credential_refresh_provider("anthropic:fixture", &route),
            Some("anthropic")
        );
    }

    #[test]
    fn startup_model_decision_preserves_selection_and_resolved_bridge() {
        let decision = decide_interactive_startup_model("selected", "resolved", true);
        assert_eq!(decision.selected_model, "selected");
        assert_eq!(decision.bridge_model, "resolved");
        assert!(decision.provider_connected);
        assert!(!decision.use_null_bridge);
    }

    #[test]
    fn unavailable_model_selects_null_bridge() {
        let decision = decide_interactive_startup_model("selected", "resolved", false);
        assert!(!decision.provider_connected);
        assert!(decision.use_null_bridge);
    }

    #[test]
    fn profile_states_are_distinct_and_explicit() {
        let interactive = profile_state("interactive").unwrap();
        let headless = profile_state("headless").unwrap();
        let daemon = profile_state("daemon").unwrap();
        let full = profile_state("full").unwrap();
        assert_ne!(interactive.0, headless.0);
        assert_ne!(headless.1, daemon.1);
        assert!(headless.2.contains(&"tui"));
        assert!(full.1.contains(&"tui"));
        assert!(profile_state("unknown").is_err());
    }

    #[cfg(feature = "task-capsule")]
    #[test]
    fn task_capsule_identity_is_compile_derived_and_bounded() {
        assert_eq!(compiled_artifact_profile(), "task-capsule-v0");
        assert_eq!(canonical_entrypoint(), ["omegon", "run"]);
        assert!(validated_profile_state("task-capsule").is_ok());
        for incompatible in ["interactive", "headless", "daemon", "full"] {
            assert!(validated_profile_state(incompatible).is_err());
        }
    }

    #[cfg(feature = "kernel-host")]
    #[test]
    fn kernel_identity_is_compile_derived_and_exclusive() {
        assert_eq!(compiled_artifact_profile(), "kernel-host-v1");
        assert_eq!(canonical_entrypoint(), ["omegon"]);
        assert!(validated_profile_state("kernel").is_ok());
        for incompatible in ["task-capsule", "interactive", "headless", "daemon", "full"] {
            assert!(validated_profile_state(incompatible).is_err());
        }
    }

    #[cfg(all(
        feature = "tui",
        feature = "self-update",
        not(feature = "task-capsule")
    ))]
    #[test]
    fn ordinary_product_cannot_claim_task_capsule_identity() {
        let expected = if cfg!(feature = "local-embeddings") {
            "full-product-local-embeddings"
        } else {
            "full-product"
        };
        assert_eq!(compiled_artifact_profile(), expected);
        assert!(validated_profile_state("task-capsule").is_err());
    }

    #[cfg(all(
        not(feature = "task-capsule"),
        not(feature = "kernel-host"),
        not(feature = "tui"),
        not(feature = "self-update")
    ))]
    #[test]
    fn bare_no_default_build_reports_shrinking_host_identity() {
        assert_eq!(compiled_artifact_profile(), "shrinking-host");
        assert!(validated_profile_state("headless").is_ok());
        assert!(validated_profile_state("daemon").is_ok());
        assert!(validated_profile_state("interactive").is_err());
        assert!(validated_profile_state("full").is_err());
    }
}
