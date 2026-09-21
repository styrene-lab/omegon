//! EventBus — typed coordination layer between the agent loop and features.
//!
//! The bus is the backbone of feature integration. Events flow down from the
//! agent loop to features; requests flow up from features to the runtime.
//!
//! ```text
//! Agent Loop
//!   │
//!   ├─emit(BusEvent)──→ EventBus ──deliver──→ Feature::on_event(&mut self)
//!   │                       │                          │
//!   │                       │                  BusRequest (accumulated)
//!   │                       │                          │
//!   │                       ←── drain_requests() ──────┘
//!   │
//!   └─ handle requests (inject message, notify, compact)
//! ```
//!
//! # Concurrency model
//!
//! The bus is NOT thread-safe. It lives in the agent loop task and processes
//! events synchronously. Features get `&mut self` — no interior mutability
//! needed. The TUI receives events via a separate `tokio::broadcast` channel.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult, ContextInjection, ContextSignals,
    Feature, ToolDefinition,
};
use serde_json::Value;

/// Core tools that are always present regardless of lazy injection.
/// These are the coding-loop essentials and the Workbench reconciliation tool
/// required by durable harness instructions on every actionable turn.
fn is_core_tool(name: &str) -> bool {
    use crate::tool_registry as reg;
    matches!(
        name,
        reg::core::BASH
            | reg::core::READ
            | reg::core::WRITE
            | reg::core::EDIT
            | reg::core::VALIDATE
            | reg::core::COMMIT
            | reg::core::TERMINAL
            | reg::core::PLAN
            | reg::codescan::CODEBASE_SEARCH
            | reg::context::CONTEXT_STATUS
            | reg::context::REQUEST_CONTEXT
            | reg::manage_tools::MANAGE_TOOLS
            | reg::view::VIEW
    )
}

/// Dynamically registered tools come from runtime-discovered surfaces such as
/// native extensions, MCP servers, and plugin manifests. Keep them visible after
/// turn 1 so operators can ask for an installed extension by name without first
/// forcing a `manage_tools` or exact tool call.
fn is_dynamic_tool(name: &str) -> bool {
    !crate::tool_registry::all_static_names().contains(&name)
}

/// Tools registered in the runtime but hidden from the model-facing tool surface.
fn is_model_hidden_tool(name: &str) -> bool {
    use crate::tool_registry as reg;
    matches!(name, reg::core::CHANGE)
}

/// Strip `description` fields from tool parameter schemas to reduce token overhead.
/// Preserves type, enum, required, default, items — the structural information
/// models need to form correct tool calls. Reduces schema tokens by ~30-40%.
fn compact_tool_schema(def: &ToolDefinition) -> ToolDefinition {
    fn strip_descriptions(val: &Value) -> Value {
        match val {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (key, value) in map {
                    if key == "description" {
                        continue; // schema annotation at this level
                    }
                    if key == "properties" {
                        // Property names are user-defined identifiers. A property
                        // literally named `description` is not a schema annotation
                        // and must survive compaction (Moonshot validates required
                        // names against this map).
                        let properties = match value {
                            Value::Object(properties) => Value::Object(
                                properties
                                    .iter()
                                    .map(|(name, schema)| {
                                        (name.clone(), strip_descriptions(schema))
                                    })
                                    .collect(),
                            ),
                            other => strip_descriptions(other),
                        };
                        out.insert(key.clone(), properties);
                    } else {
                        out.insert(key.clone(), strip_descriptions(value));
                    }
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(strip_descriptions).collect()),
            other => other.clone(),
        }
    }

    ToolDefinition {
        name: def.name.clone(),
        label: def.label.clone(),
        // Keep the top-level tool description (model needs to know what the tool does)
        // but strip parameter-level descriptions (model can infer from param names + types)
        description: def.description.clone(),
        parameters: strip_descriptions(&def.parameters),
        capabilities: def.capabilities.clone(),
    }
}

/// Default tool execution timeout (5 minutes).
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(300);
const BASH_TOOL_NAME: &str = "bash";

enum PendingFeatureMutation {
    Register(Box<dyn Feature>),
    Replace(Box<dyn Feature>),
}

impl PendingFeatureMutation {
    fn feature(&self) -> &dyn Feature {
        match self {
            Self::Register(feature) | Self::Replace(feature) => feature.as_ref(),
        }
    }
}

fn stable_feature_component(name: &str) -> String {
    let mut encoded = String::new();
    for byte in name.as_bytes() {
        let ch = char::from(*byte);
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.') {
            encoded.push(ch);
        } else {
            use std::fmt::Write as _;
            write!(&mut encoded, "_{byte:02x}").expect("writing to String cannot fail");
        }
    }
    if encoded.is_empty() {
        "empty".into()
    } else {
        encoded
    }
}

fn new_composition_generation_id() -> omegon_traits::RuntimeCompositionGenerationId {
    omegon_traits::RuntimeCompositionGenerationId::new(format!(
        "composition:{}",
        uuid::Uuid::new_v4()
    ))
    .expect("UUID composition generation is valid")
}

fn conservative_mutation_fence(name: &str) -> omegon_traits::RuntimeMutationFence {
    omegon_traits::RuntimeMutationFence {
        domain: omegon_traits::RuntimeMutationDomainId::new("workspace:runtime")
            .expect("static mutation domain is valid"),
        key: omegon_traits::RuntimeMutationFenceKey::new(format!(
            "capability:{}",
            stable_feature_component(name)
        ))
        .expect("encoded capability name is a valid fence key"),
    }
}

fn adapted_tool_effects(definition: &ToolDefinition) -> Vec<omegon_traits::RuntimeEffect> {
    use omegon_traits::{RuntimeEffect, ToolCapability};

    if definition.capabilities.is_empty() {
        return vec![
            RuntimeEffect::FilesystemRead,
            RuntimeEffect::FilesystemWrite,
            RuntimeEffect::ProcessSpawn,
            RuntimeEffect::NetworkAccess,
            RuntimeEffect::SecretDelivery,
            RuntimeEffect::TerminalAccess,
            RuntimeEffect::DurableStateWrite,
            RuntimeEffect::RuntimeControl,
        ];
    }
    let mut effects = BTreeSet::new();
    for capability in &definition.capabilities {
        match capability {
            ToolCapability::Orientation
            | ToolCapability::BroadOrientation
            | ToolCapability::RepoInspection
            | ToolCapability::BroadRepoInspection
            | ToolCapability::TargetedRepoInspection => {
                effects.insert(RuntimeEffect::FilesystemRead);
                // Existing inspection/orientation adapters may shell out to
                // VCS/provider CLIs or probe local HTTP services. Until each
                // feature supplies narrower native declarations, retain the
                // conservative host-effect envelope.
                effects.insert(RuntimeEffect::ProcessSpawn);
                effects.insert(RuntimeEffect::NetworkAccess);
                effects.insert(RuntimeEffect::SecretDelivery);
            }
            ToolCapability::Mutation | ToolCapability::StateChanging => effects.extend([
                RuntimeEffect::FilesystemRead,
                RuntimeEffect::FilesystemWrite,
                RuntimeEffect::ProcessSpawn,
                RuntimeEffect::NetworkAccess,
                RuntimeEffect::SecretDelivery,
                RuntimeEffect::TerminalAccess,
                RuntimeEffect::DurableStateWrite,
                RuntimeEffect::RuntimeControl,
            ]),
            ToolCapability::Validation => {
                effects.insert(RuntimeEffect::FilesystemRead);
                effects.insert(RuntimeEffect::ProcessSpawn);
            }
            ToolCapability::ProgressBoundary => {
                effects.insert(RuntimeEffect::RuntimeControl);
            }
        }
    }
    effects.into_iter().collect()
}

fn adapted_tool_policy(definition: &ToolDefinition) -> omegon_traits::RuntimeToolPolicy {
    use omegon_traits::{
        RuntimeDeduplication, RuntimeExecutionPolicy, RuntimeIdempotency, RuntimeParallelism,
        RuntimePrincipalClass, RuntimeRetryClass, RuntimeTimeoutClass, RuntimeTransactionBehavior,
        ToolCapability,
    };

    let effects = adapted_tool_effects(definition);
    let mutates = effects.iter().any(|effect| {
        matches!(
            effect,
            omegon_traits::RuntimeEffect::FilesystemWrite
                | omegon_traits::RuntimeEffect::DurableStateWrite
                | omegon_traits::RuntimeEffect::RuntimeControl
        )
    });
    let rollback = definition.capabilities.contains(&ToolCapability::Mutation);
    omegon_traits::RuntimeToolPolicy {
        effects,
        execution: RuntimeExecutionPolicy {
            principals: vec![RuntimePrincipalClass::Model],
            timeout_class: RuntimeTimeoutClass::Interactive,
            retry_class: RuntimeRetryClass::Never,
            idempotency: RuntimeIdempotency::NonIdempotent,
            deduplication: RuntimeDeduplication::Unsupported,
            parallelism: RuntimeParallelism::Serial,
            transaction: if rollback {
                RuntimeTransactionBehavior::BestEffortRollback
            } else if mutates {
                RuntimeTransactionBehavior::IndependentMutation
            } else {
                RuntimeTransactionBehavior::None
            },
            mutation_fence: mutates
                .then(|| Box::new(conservative_mutation_fence(&definition.name))),
            max_attempts: None,
        },
    }
}

fn adapted_command_effects(definition: &CommandDefinition) -> Vec<omegon_traits::RuntimeEffect> {
    use omegon_traits::{CommandSafetyClass, RuntimeEffect};

    match definition.safety.class {
        CommandSafetyClass::LocalOnly | CommandSafetyClass::ReadOnly => vec![],
        CommandSafetyClass::QueueMutation => vec![RuntimeEffect::RuntimeControl],
        CommandSafetyClass::StateChanging => vec![
            RuntimeEffect::DurableStateWrite,
            RuntimeEffect::RuntimeControl,
        ],
        CommandSafetyClass::ExternalSideEffect => vec![
            RuntimeEffect::NetworkAccess,
            RuntimeEffect::ProcessSpawn,
            RuntimeEffect::RuntimeControl,
        ],
        CommandSafetyClass::Destructive => vec![
            RuntimeEffect::FilesystemWrite,
            RuntimeEffect::DurableStateWrite,
            RuntimeEffect::RuntimeControl,
        ],
    }
}

fn adapted_command_surfaces(definition: &CommandDefinition) -> Vec<omegon_traits::RuntimeSurface> {
    let mut surfaces = Vec::new();
    if definition.availability.tui {
        surfaces.push(omegon_traits::RuntimeSurface::Tui);
    }
    if definition.availability.cli {
        surfaces.push(omegon_traits::RuntimeSurface::Cli);
    }
    if definition.availability.acp {
        surfaces.push(omegon_traits::RuntimeSurface::Acp);
    }
    surfaces
}

fn conservative_external_effects() -> Vec<omegon_traits::RuntimeEffect> {
    use omegon_traits::RuntimeEffect;
    vec![
        RuntimeEffect::FilesystemRead,
        RuntimeEffect::FilesystemWrite,
        RuntimeEffect::ProcessSpawn,
        RuntimeEffect::NetworkAccess,
        RuntimeEffect::SecretDelivery,
        RuntimeEffect::TerminalAccess,
        RuntimeEffect::DurableStateWrite,
        RuntimeEffect::RuntimeControl,
    ]
}

fn requested_tool_timeout(args: &Value) -> Option<u64> {
    args.get("timeout_secs")
        .and_then(Value::as_u64)
        .or_else(|| args.get("timeout").and_then(Value::as_u64))
}

fn effective_tool_timeout(
    tool_name: &str,
    args: &Value,
    default_timeout: Duration,
) -> Option<Duration> {
    match requested_tool_timeout(args) {
        Some(seconds) => Some(Duration::from_secs(seconds.saturating_add(5))),
        None if tool_name == BASH_TOOL_NAME => None,
        None => Some(default_timeout),
    }
}

#[derive(Clone)]
struct PublishedInProcessService {
    owner: omegon_traits::RuntimeContributionId,
    generation_id: omegon_traits::RuntimeContributionGenerationId,
    interface_id: omegon_traits::RuntimeServiceInterfaceId,
    implementation: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    implementation_identity: usize,
    implementation_type_id: std::any::TypeId,
}

struct FrozenFeature {
    contribution_id: omegon_traits::RuntimeContributionId,
    feature_index: usize,
    name: String,
    provenance: omegon_traits::ToolProvenance,
    tools: Vec<ToolDefinition>,
    commands: Vec<CommandDefinition>,
    acp_invocations: Vec<omegon_traits::RuntimeAcpInvocationDefinition>,
    command_aliases: Vec<omegon_traits::CommandAlias>,
    internal_tools: Vec<String>,
    tool_policies: BTreeMap<String, omegon_traits::RuntimeToolPolicy>,
    tool_surfaces: BTreeMap<String, Vec<omegon_traits::RuntimeSurface>>,
    tool_principals: BTreeMap<String, Vec<omegon_traits::RuntimePrincipalClass>>,
    command_surfaces: BTreeMap<String, Vec<omegon_traits::RuntimeSurface>>,
    lifecycle: Option<omegon_traits::RuntimeLifecyclePolicy>,
    composition_transition: Option<omegon_traits::RuntimeCompositionTransitionPolicy>,
    services: Vec<omegon_traits::RuntimeInProcessService>,
    dependencies: Vec<omegon_traits::RuntimeContributionDependency>,
    generation_id: omegon_traits::RuntimeContributionGenerationId,
}

struct PreparedComposition {
    tool_defs: Vec<(usize, ToolDefinition)>,
    command_defs: Vec<(usize, CommandDefinition)>,
    internal_tool_owners: HashMap<String, usize>,
    acp_invocation_owners: HashMap<String, usize>,
    in_process_services: BTreeMap<omegon_traits::RuntimeCapabilityId, PublishedInProcessService>,
    diagnostics: Vec<omegon_traits::RuntimeContributionDiagnostic>,
    graph: crate::contribution_graph::RuntimeCandidateGraph,
    generation_id: omegon_traits::RuntimeCompositionGenerationId,
}

pub(crate) struct PreparedDynamicPublication(PreparedComposition);

#[derive(Clone)]
pub(crate) struct InProcessServiceHandle<T: ?Sized> {
    pub(crate) capability_id: omegon_traits::RuntimeCapabilityId,
    pub(crate) owner: omegon_traits::RuntimeContributionId,
    pub(crate) generation_id: omegon_traits::RuntimeContributionGenerationId,
    pub(crate) service: std::sync::Arc<T>,
}

/// The event bus — owns all features and dispatches events to them.
pub struct EventBus {
    contribution_health: crate::contribution_health::ContributionHealth,
    project_root: std::path::PathBuf,
    features: Vec<Box<dyn Feature>>,
    /// Accumulated requests from the most recent event delivery.
    pending_requests: Vec<BusRequest>,
    /// Cached tool definitions — rebuilt when features change.
    tool_defs: Vec<(usize, ToolDefinition)>, // (feature_index, def)
    /// Cached command definitions.
    command_defs: Vec<(usize, CommandDefinition)>,
    /// Handle to the disabled tools set from ManageTools.
    tool_admission: Option<crate::features::manage_tools::SharedToolAdmissionPolicy>,
    /// Boot-policy exclusions that runtime tool management cannot override.
    policy_denied_tools: std::collections::BTreeSet<omegon_traits::RuntimeCapabilityId>,
    /// Handle to the registered tool inventory from ManageTools.
    tool_inventory: Option<crate::features::manage_tools::ToolInventory>,
    /// Per-tool execution timeouts. Tools not listed use DEFAULT_TOOL_TIMEOUT.
    tool_timeouts: HashMap<String, Duration>,
    /// Internal tool owners — maps tool names that may NOT be in tool_defs
    /// (because they're not LLM-visible) to the feature index that handles them.
    /// Populated explicitly via `register_internal_tool`.
    internal_tool_owners: HashMap<String, usize>,
    /// ACP transport invocation owners derived from the accepted graph.
    acp_invocation_owners: HashMap<String, usize>,
    /// Bindingless typed services atomically published with the accepted graph.
    in_process_services: BTreeMap<omegon_traits::RuntimeCapabilityId, PublishedInProcessService>,
    /// Resource-bearing services with generation-owned admission and cleanup.
    managed_services: crate::managed_service_bus::ManagedServiceBus,
    runtime_ownership_retention: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Structurally validated composition from which legacy caches were built.
    accepted_graph: Option<std::sync::Arc<crate::contribution_graph::RuntimeCandidateGraph>>,
    /// Identity of the atomically published composition represented by the graph and caches.
    accepted_generation_id: Option<omegon_traits::RuntimeCompositionGenerationId>,
    /// Coded diagnostics from the latest accepted or rejected candidate.
    composition_diagnostics: Vec<omegon_traits::RuntimeContributionDiagnostic>,
    /// Number of features in the active published composition.
    published_feature_count: usize,
    /// Candidate changes remain invisible until graph validation succeeds.
    pending_features: Vec<PendingFeatureMutation>,
    /// Internal binding declarations retained without last-writer arbitration.
    declared_internal_tools: Vec<(String, String)>,
    pending_internal_tools: Vec<(String, String)>,
    /// Synthetic resource-bearing generations staged by owning feature name.
    pending_managed_generations:
        BTreeMap<String, Vec<crate::managed_service_bus::ManagedGenerationCandidate>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            contribution_health: Default::default(),
            project_root: std::env::current_dir().unwrap_or_default(),
            features: Vec::new(),
            pending_requests: Vec::new(),
            tool_defs: Vec::new(),
            command_defs: Vec::new(),
            tool_admission: None,
            policy_denied_tools: std::collections::BTreeSet::new(),
            internal_tool_owners: HashMap::new(),
            acp_invocation_owners: HashMap::new(),
            in_process_services: BTreeMap::new(),
            managed_services: crate::managed_service_bus::ManagedServiceBus::default(),
            runtime_ownership_retention: None,
            accepted_graph: None,
            accepted_generation_id: None,
            composition_diagnostics: Vec::new(),
            published_feature_count: 0,
            pending_features: Vec::new(),
            declared_internal_tools: Vec::new(),
            pending_internal_tools: Vec::new(),
            pending_managed_generations: BTreeMap::new(),
            tool_inventory: None,
            tool_timeouts: HashMap::from([
                ("bash".into(), Duration::from_secs(600)),
                ("web_search".into(), Duration::from_secs(30)),
                ("web_fetch".into(), Duration::from_secs(60)),
            ]),
        }
    }

    pub(crate) fn contribution_health(&self) -> crate::contribution_health::ContributionHealth {
        self.contribution_health.clone()
    }

    pub(crate) fn set_project_root(&mut self, root: std::path::PathBuf) {
        self.project_root = root;
    }

    pub(crate) fn project_root(&self) -> &std::path::Path {
        &self.project_root
    }

    pub(crate) fn in_process_service<T>(
        &self,
        capability_id: &omegon_traits::RuntimeCapabilityId,
        interface_id: &omegon_traits::RuntimeServiceInterfaceId,
    ) -> anyhow::Result<Option<InProcessServiceHandle<T>>>
    where
        T: ?Sized + std::any::Any + Send + Sync,
    {
        let Some(published) = self.in_process_services.get(capability_id) else {
            return Ok(None);
        };
        if &published.interface_id != interface_id {
            anyhow::bail!(
                "in-process service {} exposes interface {}, not {}",
                capability_id.as_str(),
                published.interface_id.as_str(),
                interface_id.as_str()
            );
        }
        let service = published
            .implementation
            .downcast_ref::<omegon_traits::RuntimeServiceHolder<T>>()
            .map(|holder| std::sync::Arc::clone(&holder.service))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "in-process service {} has an incompatible implementation type for interface {}",
                    capability_id.as_str(),
                    interface_id.as_str()
                )
            })?;
        Ok(Some(InProcessServiceHandle {
            capability_id: capability_id.clone(),
            owner: published.owner.clone(),
            generation_id: published.generation_id.clone(),
            service,
        }))
    }

    pub(crate) async fn publish_managed_service<S>(
        &mut self,
        candidate: crate::managed_service_bus::ManagedServiceCandidate<S>,
    ) -> crate::managed_service_bus::ManagedServicePublicationOutcome
    where
        S: omegon_traits::ManagedServiceContract + ?Sized,
    {
        if self.accepted_graph.as_ref().is_some_and(|graph| {
            graph
                .capability_owners
                .contains_key(candidate.capability_id())
        }) {
            let capability_id = candidate.capability_id().clone();
            let reason = format!(
                "direct managed service {} collides with the accepted contribution graph",
                capability_id.as_str()
            );
            let candidates = vec![candidate.into()];
            let mut cleanup = self
                .managed_services
                .reject_composition_candidates(&candidates, &reason)
                .await;
            return crate::managed_service_bus::ManagedServicePublicationOutcome::Rejected {
                reason,
                cleanup: cleanup.pop(),
            };
        }
        self.managed_services.publish(candidate).await
    }

    pub(crate) fn stage_managed_generation(
        &mut self,
        feature_name: impl Into<String>,
        candidate: crate::managed_service_bus::ManagedGenerationCandidate,
    ) -> anyhow::Result<()> {
        self.pending_managed_generations
            .entry(feature_name.into())
            .or_default()
            .push(candidate);
        Ok(())
    }

    pub(crate) fn managed_service<S>(
        &self,
        capability_id: &omegon_traits::RuntimeCapabilityId,
        interface_id: &omegon_traits::RuntimeServiceInterfaceId,
    ) -> anyhow::Result<Option<crate::service_generation::ManagedServiceHandle<S>>>
    where
        S: omegon_traits::ManagedServiceContract + ?Sized,
    {
        self.managed_services.service(capability_id, interface_id)
    }

    pub(crate) fn managed_service_metadata(
        &self,
    ) -> Vec<crate::managed_service_bus::ManagedPublishedServiceMetadata> {
        self.managed_services.published_metadata()
    }

    pub(crate) fn active_startup_resource_owners(&self) -> BTreeMap<String, usize> {
        self.managed_services.active_resource_owners()
    }

    pub(crate) async fn shutdown_managed_services(
        &mut self,
    ) -> crate::managed_service_bus::ManagedServiceShutdownReport {
        let mut feature_failures = Vec::new();
        for feature in &mut self.features {
            if let Err(error) = feature.prepare_managed_shutdown().await {
                let failure = format!("{}: {error:#}", feature.name());
                tracing::warn!(feature = feature.name(), %error, "feature pre-shutdown failed");
                feature_failures.push(failure);
            }
        }
        let mut report = self.managed_services.shutdown().await;
        report.feature_failures = feature_failures;
        for generation in &report.generations {
            match &generation.result {
                Ok(cleanup) if cleanup.resources.all_resources_settled() => {}
                Ok(cleanup) => tracing::warn!(
                    owner = generation.owner.as_str(),
                    generation = generation.generation_id.as_str(),
                    resources = cleanup.resources.records.len(),
                    "managed generation shutdown remains unsettled"
                ),
                Err(error) => tracing::warn!(
                    owner = generation.owner.as_str(),
                    generation = generation.generation_id.as_str(),
                    %error,
                    "managed generation shutdown failed"
                ),
            }
        }
        for cleanup in &report.rejected_candidates {
            if !cleanup.all_resources_settled() {
                tracing::warn!(
                    resources = cleanup.records.len(),
                    "rejected managed candidate shutdown remains unsettled"
                );
            }
        }
        if let Some(retention) = &self.runtime_ownership_retention {
            retention.store(
                !report.all_resources_settled(),
                std::sync::atomic::Ordering::Release,
            );
        }
        report
    }

    pub(crate) async fn shutdown_managed_services_strict(&mut self) -> anyhow::Result<()> {
        let report = self.shutdown_managed_services().await;
        if report.all_resources_settled() {
            Ok(())
        } else {
            anyhow::bail!("managed-service cleanup did not settle: {report:?}")
        }
    }

    pub(crate) fn bind_runtime_ownership_retention(
        &mut self,
        retention: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        self.runtime_ownership_retention = Some(retention);
    }

    pub fn apply_operator_tool_profile(
        &mut self,
        slim_mode: bool,
        posture_disabled: &[String],
        posture_enabled: &[String],
    ) {
        use crate::tool_registry as reg;
        let Some(handle) = self.tool_admission.as_ref() else {
            return;
        };
        let mut disabled = handle.lock().unwrap();
        disabled.clear();

        // ── Base defaults (all modes): situational tools hidden from the
        // default schema. The agent discovers them via lazy injection if
        // it calls one after seeing the full surface on turn 1.
        disabled.insert(reg::persona::SWITCH_PERSONA.into());
        disabled.insert(reg::persona::SWITCH_TONE.into());
        disabled.insert(reg::persona::LIST_PERSONAS.into());
        disabled.insert(reg::auth::AUTH_STATUS.into());
        disabled.insert(reg::harness_settings::HARNESS_SETTINGS.into());
        disabled.insert(reg::memory::MEMORY_INGEST_LIFECYCLE.into());
        disabled.insert(reg::memory::MEMORY_CONNECT.into());
        disabled.insert(reg::memory::MEMORY_SEARCH_ARCHIVE.into());
        disabled.insert(reg::lifecycle::OPENSPEC_MANAGE.into());
        disabled.insert(reg::lifecycle::LIFECYCLE_DOCTOR.into());
        disabled.insert(reg::codescan::CODEBASE_INDEX.into());
        disabled.insert(reg::session_log::SESSION_LOG.into());
        disabled.insert(reg::model_budget::SET_MODEL_INTENT.into());
        disabled.insert(reg::model_budget::SWITCH_TO_OFFLINE_DRIVER.into());
        disabled.insert(reg::model_budget::SET_THINKING_LEVEL.into());

        if slim_mode {
            // Slim/explorator: additionally suppress delegation, orchestration,
            // lifecycle surfaces, and heavyweight tools beyond the base
            // defaults.  Hiding design_tree and openspec from the tool
            // list means the LLM cannot reference concepts the operator
            // hasn't learned yet (Cruise zone — see
            // design/junior-onramp-progressive-disclosure.md).
            disabled.insert(reg::delegate::DELEGATE.into());
            disabled.insert(reg::delegate::DELEGATE_RESULT.into());
            disabled.insert(reg::delegate::DELEGATE_STATUS.into());
            disabled.insert(reg::cleave::CLEAVE_ASSESS.into());
            disabled.insert(reg::cleave::CLEAVE_RUN.into());
            disabled.insert(reg::lifecycle::DESIGN_TREE.into());
            disabled.insert(reg::lifecycle::DESIGN_TREE_UPDATE.into());
            disabled.insert(reg::lifecycle::OPENSPEC_MANAGE.into());
            disabled.insert(reg::local_inference::LIST_LOCAL_MODELS.into());
            disabled.insert(reg::local_inference::MANAGE_OLLAMA.into());
            disabled.insert(reg::core::SERVE.into());
            disabled.insert(reg::view::VIEW.into());
            disabled.insert(reg::context::CONTEXT_COMPACT.into());
            disabled.insert(reg::context::CONTEXT_CLEAR.into());
        }

        // Custom posture tool overrides
        for tool in posture_disabled {
            disabled.insert(tool.clone());
        }

        // Whitelist mode: if posture_enabled is non-empty, disable everything
        // except the listed tools. This is applied last so it overrides all
        // other disable/enable decisions.
        if !posture_enabled.is_empty() {
            let all_tools: Vec<String> =
                self.tool_defs.iter().map(|(_, d)| d.name.clone()).collect();
            for tool in &all_tools {
                if !posture_enabled.contains(tool) {
                    disabled.insert(tool.clone());
                }
            }
            // Ensure enabled tools are NOT in the disabled set
            for tool in posture_enabled {
                disabled.remove(tool);
            }
        }
    }

    /// Set the disabled tools handle (called from setup after ManageTools is registered).
    pub fn set_tool_admission_policy(
        &mut self,
        handle: crate::features::manage_tools::SharedToolAdmissionPolicy,
    ) {
        self.tool_admission = Some(handle);
    }

    pub(crate) fn set_policy_denied_tools<I, S>(&mut self, tools: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.policy_denied_tools = tools
            .into_iter()
            .map(|tool| omegon_traits::RuntimeCapabilityId::tool(tool.as_ref()))
            .collect();
        self.refresh_tool_inventory();
    }

    fn is_tool_denied_by_policy(&self, tool_name: &str) -> bool {
        self.policy_denied_tools
            .contains(&omegon_traits::RuntimeCapabilityId::tool(tool_name))
    }

    /// Set the ManageTools inventory handle so finalize can keep its list in
    /// sync with the bus's current model-visible tool cache.
    pub fn set_tool_inventory(&mut self, handle: crate::features::manage_tools::ToolInventory) {
        self.tool_inventory = Some(handle);
        self.refresh_tool_inventory();
    }

    /// Register a feature. Call during setup before the agent loop starts.
    pub fn register(&mut self, feature: Box<dyn Feature>) {
        tracing::info!(feature = feature.name(), "registered feature");
        if self.accepted_graph.is_some() {
            self.pending_features
                .push(PendingFeatureMutation::Register(feature));
        } else {
            self.features.push(feature);
        }
    }

    /// Replace an existing feature by name, or register it if absent.
    /// Call `finalize()` afterwards to rebuild cached command/tool definitions.
    pub fn replace_feature(&mut self, feature: Box<dyn Feature>) {
        let name = feature.name().to_string();
        if self.accepted_graph.is_some() {
            tracing::info!(feature = %name, "staged replacement feature");
            self.pending_features
                .push(PendingFeatureMutation::Replace(feature));
            return;
        }
        if let Some(idx) = self.features.iter().position(|f| f.name() == name) {
            tracing::info!(feature = %name, "replaced feature");
            self.features[idx] = feature;
        } else {
            tracing::info!(feature = %name, "registered feature");
            self.features.push(feature);
        }
    }

    /// Register an internal tool name → feature mapping. Internal tools
    /// are NOT in the LLM-visible tool_defs but can be called via
    /// `execute_internal`. The feature_name must match a previously
    /// registered feature.
    pub fn register_internal_tool(&mut self, tool_name: &str, feature_name: &str) {
        let feature_exists = self
            .features
            .iter()
            .any(|feature| feature.name() == feature_name)
            || self
                .pending_features
                .iter()
                .any(|pending| pending.feature().name() == feature_name);
        if feature_exists {
            tracing::debug!(
                tool = tool_name,
                feature = feature_name,
                "staged internal tool"
            );
            let binding = (tool_name.to_string(), feature_name.to_string());
            if self.accepted_graph.is_some() {
                self.pending_internal_tools.push(binding);
            } else {
                self.declared_internal_tools.push(binding);
            }
        } else {
            tracing::warn!(
                tool = tool_name,
                feature = feature_name,
                "cannot register internal tool — feature not found"
            );
        }
    }

    /// Validate the staged feature set and publish graph-derived legacy caches.
    pub fn finalize(&mut self) {
        self.try_finalize()
            .expect("staged EventBus composition must validate before publication");
    }

    pub(crate) fn composition_generation_id(
        &self,
    ) -> Option<&omegon_traits::RuntimeCompositionGenerationId> {
        self.accepted_generation_id.as_ref()
    }

    pub(crate) fn composition_diagnostic_projection(
        &mut self,
    ) -> Option<crate::surfaces::diagnostics::CompositionDiagnosticProjection> {
        use crate::surfaces::diagnostics::{
            CompatibilityDispatchMode, CompatibilityDispatchProjection,
            CompositionContributionProjection, CompositionDiagnosticProjection,
            CompositionReplacementProjection,
        };
        use omegon_traits::{
            RuntimeCleanupAssurance, RuntimeCleanupRequirement, RuntimeCleanupState,
            RuntimeContributionLifecycleState,
        };

        let managed_owners = match self.managed_services.managed_diagnostic_records() {
            Ok(records) => records,
            Err(error) => {
                tracing::warn!(%error, "managed diagnostic reconciliation failed");
                Vec::new()
            }
        };
        let graph_managed = self
            .managed_services
            .graph_managed_identities()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let graph = self.accepted_graph.as_ref()?;
        let generation_id = self.accepted_generation_id.clone()?;
        let contributions = graph
            .declarations
            .values()
            .cloned()
            .map(|declaration| {
                let negotiated_protocol = graph
                    .negotiated_protocols
                    .get(&declaration.id)
                    .copied()
                    .unwrap_or(declaration.protocol.minimum);
                let cleanup_assurance = match declaration.transition.cleanup {
                    RuntimeCleanupRequirement::Strict => RuntimeCleanupAssurance::Strict,
                    RuntimeCleanupRequirement::BestEffort => RuntimeCleanupAssurance::BestEffort,
                };
                let managed_lifecycle = graph_managed
                    .contains(&(declaration.id.clone(), declaration.generation_id.clone()))
                    .then(|| {
                        managed_owners.iter().rev().find(|owner| {
                            owner.disposition
                                == crate::surfaces::diagnostics::ManagedOwnerDisposition::Published
                                && owner.lifecycle.contribution_id == declaration.id
                                && owner.lifecycle.generation_id == declaration.generation_id
                        })
                    })
                    .flatten();
                CompositionContributionProjection {
                    declaration,
                    negotiated_protocol,
                    health: managed_lifecycle
                        .map_or(RuntimeContributionLifecycleState::Active, |owner| {
                            owner.lifecycle.state
                        }),
                    cleanup_assurance: managed_lifecycle
                        .map_or(cleanup_assurance, |owner| owner.lifecycle.cleanup_assurance),
                    cleanup_state: managed_lifecycle
                        .map_or(RuntimeCleanupState::NotRequired, |owner| {
                            owner.lifecycle.cleanup_state
                        }),
                }
            })
            .collect();
        let replacements = graph
            .superseded
            .iter()
            .map(
                |(superseded, replacement)| CompositionReplacementProjection {
                    superseded: superseded.clone(),
                    replacement: replacement.clone(),
                },
            )
            .collect();

        Some(CompositionDiagnosticProjection {
            version: crate::surfaces::diagnostics::DIAGNOSTIC_PROJECTION_VERSION,
            generation_id,
            contributions,
            replacements,
            activation_waves: graph.activation_waves.clone(),
            diagnostics: self.composition_diagnostics.clone(),
            compatibility_dispatch: CompatibilityDispatchProjection {
                mode: CompatibilityDispatchMode::GraphDerivedLegacy,
                parity_verified: true,
                published_bindings: graph.invocation_owners.len(),
            },
            managed_owners,
        })
    }

    /// Fallible publication boundary used by production setup.
    pub(crate) fn try_finalize(&mut self) -> anyhow::Result<()> {
        if !self.pending_managed_generations.is_empty()
            || self.managed_services.has_graph_managed_generations()
        {
            anyhow::bail!(
                "managed generations are staged or active; use EventBus::try_finalize_managed().await"
            );
        }
        let generation_id = new_composition_generation_id();
        let prepared = self.prepare_finalization(&BTreeMap::new(), generation_id, false);
        match prepared {
            Ok(prepared) => {
                self.commit_finalization(prepared);
                Ok(())
            }
            Err(error) => {
                self.clear_pending_ordinary();
                Err(error)
            }
        }
    }

    pub(crate) fn prepare_dynamic_publication(
        &mut self,
    ) -> anyhow::Result<PreparedDynamicPublication> {
        let generation_id = new_composition_generation_id();
        match self.prepare_finalization(&BTreeMap::new(), generation_id, true) {
            Ok(prepared) => Ok(PreparedDynamicPublication(prepared)),
            Err(error) => {
                self.clear_pending_ordinary();
                Err(error)
            }
        }
    }

    pub(crate) fn commit_dynamic_publication(&mut self, publication: PreparedDynamicPublication) {
        self.commit_finalization(publication.0);
    }

    pub(crate) async fn try_finalize_managed(&mut self) -> anyhow::Result<()> {
        let mut candidates = std::mem::take(&mut self.pending_managed_generations);
        let generation_id = new_composition_generation_id();
        let prepared = match self.prepare_finalization(&candidates, generation_id.clone(), false) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.clear_pending_ordinary();
                let rejection_reason = error.to_string();
                let cleanup = self
                    .managed_services
                    .reject_composition_candidates(
                        &candidates.into_values().flatten().collect::<Vec<_>>(),
                        &rejection_reason,
                    )
                    .await;
                return Err(anyhow::anyhow!(
                    "{error}; managed candidate rollback cleanup reports: {cleanup:?}"
                ));
            }
        };

        let should_publish =
            !candidates.is_empty() || self.managed_services.has_graph_managed_generations();
        let publication = if should_publish {
            for (feature_name, feature_candidates) in &mut candidates {
                let candidate = feature_candidates
                    .first_mut()
                    .expect("prepared managed owner has one candidate");
                let contribution_id = omegon_traits::RuntimeContributionId::new(format!(
                    "feature:{}",
                    stable_feature_component(feature_name)
                ))
                .expect("encoded feature identity is valid");
                let generation_id = prepared
                    .graph
                    .declarations
                    .get(&contribution_id)
                    .expect("prepared managed owner exists")
                    .generation_id
                    .clone();
                candidate.rebind(
                    prepared.generation_id.clone(),
                    contribution_id,
                    generation_id,
                );
            }
            Some(
                self.managed_services
                    .publish_composition(candidates.into_values().flatten().collect())
                    .await,
            )
        } else {
            None
        };
        if let Some(crate::managed_service_bus::ManagedServiceBatchPublicationOutcome::Rejected {
            reason,
            cleanup,
        }) = publication
        {
            self.clear_pending_ordinary();
            anyhow::bail!(
                "managed composition publication rejected: {reason}; rollback cleanup reports: {cleanup:?}"
            );
        }

        // The managed admission replacement is the linearization point. Keep this
        // commit assignment-only and do not introduce suspension between the two.
        self.commit_finalization(prepared);
        Ok(())
    }

    fn prepare_finalization(
        &mut self,
        managed_candidates: &BTreeMap<
            String,
            Vec<crate::managed_service_bus::ManagedGenerationCandidate>,
        >,
        generation_id: omegon_traits::RuntimeCompositionGenerationId,
        retain_active_managed: bool,
    ) -> anyhow::Result<PreparedComposition> {
        use omegon_traits::{
            RuntimeActivationBoundary, RuntimeAuthorityNarrowing, RuntimeCapabilityGroupId,
            RuntimeCapabilityId, RuntimeCapabilityKind, RuntimeCapabilityTransitionPolicy,
            RuntimeCleanupRequirement, RuntimeCompositionTransitionPolicy,
            RuntimeConfinementRequest, RuntimeContributionCapabilityDeclaration,
            RuntimeContributionCapabilityGroup, RuntimeContributionDeclaration,
            RuntimeContributionGenerationId, RuntimeContributionSchemaVersion,
            RuntimeDeduplication, RuntimeExecutionPolicy, RuntimeFailureDisposition,
            RuntimeIdempotency, RuntimeInvocationBinding, RuntimeInvocationBindingRole,
            RuntimeLifecyclePolicy, RuntimeLifecycleRequirement, RuntimeOwnerTier,
            RuntimeParallelism, RuntimePlatformRequirements, RuntimePrincipalClass,
            RuntimeProtocolRange, RuntimeRetryClass, RuntimeSurface, RuntimeTimeoutClass,
            RuntimeTransactionBehavior, RuntimeTrustRequest,
        };

        let replaced_names = self
            .pending_features
            .iter()
            .filter_map(|pending| match pending {
                PendingFeatureMutation::Replace(feature) => Some(feature.name().to_string()),
                PendingFeatureMutation::Register(_) => None,
            })
            .collect::<BTreeSet<_>>();
        let mut planned_indices = self
            .features
            .iter()
            .enumerate()
            .map(|(index, feature)| (feature.name().to_string(), index))
            .collect::<BTreeMap<_, _>>();
        let mut next_index = self.features.len();
        let mut candidate_features = self
            .features
            .iter()
            .enumerate()
            .filter(|(_, feature)| !replaced_names.contains(feature.name()))
            .map(|(index, feature)| (index, feature.as_ref()))
            .collect::<Vec<_>>();
        for pending in &self.pending_features {
            let feature = pending.feature();
            let index = match pending {
                PendingFeatureMutation::Replace(_) => *planned_indices
                    .entry(feature.name().to_string())
                    .or_insert_with(|| {
                        let index = next_index;
                        next_index += 1;
                        index
                    }),
                PendingFeatureMutation::Register(_) => {
                    let index = next_index;
                    next_index += 1;
                    index
                }
            };
            candidate_features.push((index, feature));
        }

        let mut candidate_internal_tools = self.declared_internal_tools.clone();
        candidate_internal_tools.extend(self.pending_internal_tools.iter().cloned());
        let mut frozen = Vec::with_capacity(candidate_features.len());
        for (feature_index, feature) in candidate_features {
            let component = stable_feature_component(feature.name());
            let contribution_id =
                omegon_traits::RuntimeContributionId::new(format!("feature:{component}"))
                    .expect("encoded feature identity is a valid scoped id");
            let mut internal_tools = candidate_internal_tools
                .iter()
                .filter(|(_, owner)| owner == feature.name())
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            internal_tools.sort();
            let tools = feature.tools();
            let acp_invocations = feature.runtime_acp_invocations();
            let tool_policies = tools
                .iter()
                .filter_map(|tool| {
                    feature
                        .runtime_tool_policy(&tool.name)
                        .map(|policy| (tool.name.clone(), policy))
                })
                .collect();
            let tool_surfaces = tools
                .iter()
                .filter_map(|tool| {
                    feature
                        .runtime_tool_surfaces(&tool.name)
                        .map(|surfaces| (tool.name.clone(), surfaces))
                })
                .collect();
            let tool_principals = tools
                .iter()
                .filter_map(|tool| {
                    feature
                        .runtime_tool_principals(&tool.name)
                        .map(|principals| (tool.name.clone(), principals))
                })
                .collect();
            let commands = feature.commands();
            let command_surfaces = commands
                .iter()
                .filter_map(|command| {
                    feature
                        .runtime_command_surfaces(&command.name)
                        .map(|surfaces| (command.name.clone(), surfaces))
                })
                .collect();
            frozen.push(FrozenFeature {
                contribution_id,
                feature_index,
                name: feature.name().to_string(),
                provenance: feature.tool_provenance(),
                tools,
                commands,
                acp_invocations,
                command_aliases: feature.command_aliases(),
                internal_tools,
                tool_policies,
                tool_surfaces,
                tool_principals,
                command_surfaces,
                lifecycle: feature.runtime_lifecycle_policy(),
                composition_transition: feature.runtime_transition_policy(),
                services: feature.runtime_in_process_services(),
                dependencies: feature.runtime_dependencies(),
                generation_id: feature
                    .runtime_contribution_generation_id()
                    .unwrap_or_else(|| {
                        RuntimeContributionGenerationId::new(format!(
                            "contribution:{}-static-v1",
                            stable_feature_component(feature.name())
                        ))
                        .expect("feature names form stable generation identifiers")
                    }),
            });
        }
        frozen.sort_by_key(|feature| feature.feature_index);

        let mut staged_managed_identities = BTreeSet::new();
        for (feature_name, feature_candidates) in managed_candidates {
            if feature_candidates.len() != 1 {
                anyhow::bail!("feature {feature_name} has duplicate managed generation candidates");
            }
            let candidate = &feature_candidates[0];
            let Some(feature) = frozen.iter().find(|feature| &feature.name == feature_name) else {
                anyhow::bail!("managed generation owner feature {feature_name} does not exist");
            };
            let active_call_timeout_ms = u64::try_from(candidate.active_call_duration().as_millis())
                .ok()
                .filter(|timeout| *timeout != 0)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "managed generation for {feature_name} requires a representable nonzero active-call timeout"
                    )
                })?;
            let cleanup_timeout_ms = u64::try_from(candidate.cleanup_duration().as_millis())
                .ok()
                .filter(|timeout| *timeout != 0)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "managed generation for {feature_name} requires a representable nonzero cleanup timeout"
                    )
                })?;
            let transition = feature.composition_transition.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "managed generation owner {feature_name} requires a runtime composition transition policy"
                )
            })?;
            if transition.cleanup_timeout_ms != cleanup_timeout_ms {
                anyhow::bail!(
                    "managed generation owner {feature_name} requires cleanup timeout {cleanup_timeout_ms}ms"
                );
            }
            if transition.cleanup != candidate.cleanup_requirement() {
                anyhow::bail!(
                    "managed generation owner {feature_name} cleanup assurance does not match its resource controllers"
                );
            }
            staged_managed_identities.insert((
                feature.contribution_id.clone(),
                feature.generation_id.clone(),
            ));
            let exact = self.managed_services.is_exact_generation(
                candidate,
                &feature.contribution_id,
                &feature.generation_id,
            );
            match transition.activation_boundary {
                RuntimeActivationBoundary::ProjectionBoundary => anyhow::bail!(
                    "managed generation owner {feature_name} cannot activate at ProjectionBoundary"
                ),
                RuntimeActivationBoundary::Boot if self.accepted_graph.is_some() && !exact => {
                    anyhow::bail!(
                        "new or changed Boot managed generation for {feature_name} is rejected after composition publication"
                    )
                }
                RuntimeActivationBoundary::QuiescentSession
                    if self.accepted_graph.is_some() && !exact =>
                {
                    anyhow::bail!(
                        "new or changed QuiescentSession managed generation for {feature_name} requires a production quiescence proof issuer"
                    )
                }
                RuntimeActivationBoundary::Boot | RuntimeActivationBoundary::QuiescentSession => {}
            }
            debug_assert_ne!(active_call_timeout_ms, 0);
        }
        for active_identity in self.managed_services.graph_managed_identities() {
            let retained = retain_active_managed
                && frozen.iter().any(|feature| {
                    feature.contribution_id == active_identity.0
                        && feature.generation_id == active_identity.1
                });
            if !staged_managed_identities.contains(&active_identity) && !retained {
                anyhow::bail!(
                    "removing active managed generation {} {} requires an authorized quiescent replacement",
                    active_identity.0.as_str(),
                    active_identity.1.as_str()
                );
            }
        }
        let staged_managed_capabilities = managed_candidates
            .values()
            .flatten()
            .flat_map(|candidate| {
                candidate
                    .services()
                    .map(|(capability_id, _)| capability_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let retained_graph_managed_services = if retain_active_managed {
            self.managed_services
                .graph_managed_metadata()
                .into_iter()
                .filter(|service| !staged_managed_capabilities.contains(&service.capability_id))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let identity_check = frozen.iter().try_for_each(|feature| {
            for tool in &feature.tools {
                RuntimeCapabilityId::new(format!("tool:{}", tool.name)).map_err(|error| {
                    anyhow::anyhow!(
                        "feature {} declares invalid tool name {:?}: {error}",
                        feature.name,
                        tool.name
                    )
                })?;
            }
            for command in &feature.commands {
                RuntimeCapabilityId::new(format!("action:{}", command.name)).map_err(|error| {
                    anyhow::anyhow!(
                        "feature {} declares invalid command name {:?}: {error}",
                        feature.name,
                        command.name
                    )
                })?;
            }
            for alias in &feature.command_aliases {
                for (kind, name) in [
                    ("alias", alias.alias.as_str()),
                    ("canonical alias target", alias.canonical.as_str()),
                ] {
                    RuntimeCapabilityId::new(format!("action:{name}")).map_err(|error| {
                        anyhow::anyhow!(
                            "feature {} declares invalid {kind} {:?}: {error}",
                            feature.name,
                            name
                        )
                    })?;
                }
            }
            for internal in &feature.internal_tools {
                RuntimeCapabilityId::new(format!("internal:{internal}")).map_err(|error| {
                    anyhow::anyhow!(
                        "feature {} declares invalid internal binding {:?}: {error}",
                        feature.name,
                        internal
                    )
                })?;
            }
            Ok::<(), anyhow::Error>(())
        });
        if let Err(error) = identity_check {
            self.pending_features.clear();
            self.pending_internal_tools.clear();
            return Err(error);
        }

        let known_tools = frozen
            .iter()
            .flat_map(|feature| feature.tools.iter())
            .map(|tool| RuntimeCapabilityId::tool(&tool.name))
            .collect::<BTreeSet<_>>();
        let groups = crate::features::manage_tools::TOOL_GROUPS
            .iter()
            .filter_map(|(name, members)| {
                let members = members
                    .iter()
                    .map(|member| RuntimeCapabilityId::tool(member))
                    .filter(|member| known_tools.contains(member))
                    .collect::<Vec<_>>();
                (!members.is_empty()).then(|| RuntimeContributionCapabilityGroup {
                    id: RuntimeCapabilityGroupId::new(format!("group:{name}"))
                        .expect("built-in tool group names are stable identifiers"),
                    members,
                })
            })
            .collect::<Vec<_>>();

        let policy = |name: &str,
                      effects: &[omegon_traits::RuntimeEffect],
                      principal: RuntimePrincipalClass| {
            let idempotent = effects
                .iter()
                .all(|effect| matches!(effect, omegon_traits::RuntimeEffect::FilesystemRead));
            let mutates = effects.iter().any(|effect| {
                matches!(
                    effect,
                    omegon_traits::RuntimeEffect::FilesystemWrite
                        | omegon_traits::RuntimeEffect::DurableStateWrite
                        | omegon_traits::RuntimeEffect::RuntimeControl
                )
            });
            RuntimeExecutionPolicy {
                principals: vec![principal],
                timeout_class: RuntimeTimeoutClass::Interactive,
                retry_class: if idempotent {
                    RuntimeRetryClass::IdempotentFailure
                } else {
                    RuntimeRetryClass::Never
                },
                idempotency: if idempotent {
                    RuntimeIdempotency::Idempotent
                } else {
                    RuntimeIdempotency::NonIdempotent
                },
                deduplication: RuntimeDeduplication::Unsupported,
                parallelism: RuntimeParallelism::Serial,
                transaction: if mutates {
                    RuntimeTransactionBehavior::IndependentMutation
                } else {
                    RuntimeTransactionBehavior::None
                },
                mutation_fence: mutates.then(|| Box::new(conservative_mutation_fence(name))),
                max_attempts: None,
            }
        };
        let transition = || RuntimeCapabilityTransitionPolicy {
            authority_narrowing: RuntimeAuthorityNarrowing::CompleteExisting,
            active_call_timeout_ms: DEFAULT_TOOL_TIMEOUT.as_millis() as u64,
        };
        let mut declarations = frozen
            .iter()
            .map(|feature| {
                let external = matches!(
                    &feature.provenance,
                    omegon_traits::ToolProvenance::Extension { .. }
                );
                let alias_names = feature
                    .command_aliases
                    .iter()
                    .map(|alias| alias.alias.as_str())
                    .collect::<BTreeSet<_>>();
                let commands_by_name = feature.commands.iter().fold(
                    BTreeMap::<&str, Vec<&CommandDefinition>>::new(),
                    |mut commands, command| {
                        commands
                            .entry(command.name.as_str())
                            .or_default()
                            .push(command);
                        commands
                    },
                );
                let mut command_groups: BTreeMap<
                    String,
                    (Vec<&CommandDefinition>, Vec<RuntimeInvocationBinding>),
                > = BTreeMap::new();
                for command in &feature.commands {
                    if !alias_names.contains(command.name.as_str()) {
                        let group = command_groups.entry(command.name.clone()).or_default();
                        group.0.push(command);
                        group.1.push(RuntimeInvocationBinding {
                            kind: omegon_traits::RuntimeInvocationKind::Command,
                            name: command.name.clone(),
                            role: RuntimeInvocationBindingRole::Canonical,
                        });
                    }
                }
                for alias in &feature.command_aliases {
                    let group = command_groups.entry(alias.canonical.clone()).or_default();
                    if !group
                        .1
                        .iter()
                        .any(|binding| binding.role == RuntimeInvocationBindingRole::Canonical)
                        && let Some(canonical) = commands_by_name.get(alias.canonical.as_str())
                    {
                        for command in canonical {
                            group.0.push(command);
                            group.1.push(RuntimeInvocationBinding {
                                kind: omegon_traits::RuntimeInvocationKind::Command,
                                name: command.name.clone(),
                                role: RuntimeInvocationBindingRole::Canonical,
                            });
                        }
                    }
                    match commands_by_name.get(alias.alias.as_str()) {
                        Some(commands) => {
                            for command in commands {
                                group.0.push(command);
                                group.1.push(RuntimeInvocationBinding {
                                    kind: omegon_traits::RuntimeInvocationKind::Command,
                                    name: command.name.clone(),
                                    role: RuntimeInvocationBindingRole::Alias,
                                });
                            }
                        }
                        None => group.1.push(RuntimeInvocationBinding {
                            kind: omegon_traits::RuntimeInvocationKind::Command,
                            name: alias.alias.clone(),
                            role: RuntimeInvocationBindingRole::Alias,
                        }),
                    }
                }
                let mut capabilities =
                    feature
                        .tools
                        .iter()
                        .map(|tool| {
                            let mut tool_policy = if external {
                                let effects = conservative_external_effects();
                                omegon_traits::RuntimeToolPolicy {
                                    execution: policy(
                                        &tool.name,
                                        &effects,
                                        RuntimePrincipalClass::Model,
                                    ),
                                    effects,
                                }
                            } else {
                                feature
                                    .tool_policies
                                    .get(&tool.name)
                                    .cloned()
                                    .unwrap_or_else(|| adapted_tool_policy(tool))
                            };
                            if let Some(principals) = feature.tool_principals.get(&tool.name) {
                                tool_policy.execution.principals = principals.clone();
                            }
                            RuntimeContributionCapabilityDeclaration {
                                id: RuntimeCapabilityId::tool(&tool.name),
                                kind: RuntimeCapabilityKind::Tool,
                                service_interface: None,
                                bindings: vec![RuntimeInvocationBinding {
                                    kind: omegon_traits::RuntimeInvocationKind::Tool,
                                    name: tool.name.clone(),
                                    role: RuntimeInvocationBindingRole::Canonical,
                                }],
                                effects: tool_policy.effects,
                                execution: tool_policy.execution,
                                transition: transition(),
                                surfaces: feature
                                    .tool_surfaces
                                    .get(&tool.name)
                                    .cloned()
                                    .unwrap_or_else(|| vec![RuntimeSurface::Model]),
                            }
                        })
                        .chain(feature.acp_invocations.iter().map(|invocation| {
                            let effects = conservative_external_effects();
                            RuntimeContributionCapabilityDeclaration {
                                id: RuntimeCapabilityId::new(format!(
                                    "acp:{}",
                                    stable_feature_component(&invocation.name)
                                ))
                                .expect("encoded ACP invocation forms a stable capability id"),
                                kind: RuntimeCapabilityKind::TransportAdapter,
                                service_interface: None,
                                bindings: vec![RuntimeInvocationBinding {
                                    kind: omegon_traits::RuntimeInvocationKind::Acp,
                                    name: invocation.name.clone(),
                                    role: RuntimeInvocationBindingRole::Canonical,
                                }],
                                execution: policy(
                                    &invocation.name,
                                    &effects,
                                    RuntimePrincipalClass::Operator,
                                ),
                                effects,
                                transition: transition(),
                                surfaces: vec![RuntimeSurface::Acp],
                            }
                        }))
                        .chain(command_groups.into_iter().map(
                            |(canonical, (commands, bindings))| {
                                let effects = if external {
                                    conservative_external_effects()
                                } else {
                                    commands
                                        .iter()
                                        .flat_map(|command| adapted_command_effects(command))
                                        .collect::<BTreeSet<_>>()
                                        .into_iter()
                                        .collect::<Vec<_>>()
                                };
                                let surfaces = feature
                                    .command_surfaces
                                    .get(&canonical)
                                    .cloned()
                                    .unwrap_or_else(|| {
                                        commands
                                            .iter()
                                            .flat_map(|command| adapted_command_surfaces(command))
                                            .collect::<BTreeSet<_>>()
                                            .into_iter()
                                            .collect::<Vec<_>>()
                                    });
                                RuntimeContributionCapabilityDeclaration {
                                    id: RuntimeCapabilityId::action(&canonical),
                                    kind: RuntimeCapabilityKind::OperatorAction,
                                    service_interface: None,
                                    bindings,
                                    execution: policy(
                                        &canonical,
                                        &effects,
                                        RuntimePrincipalClass::Operator,
                                    ),
                                    effects,
                                    transition: transition(),
                                    surfaces,
                                }
                            },
                        ))
                        .collect::<Vec<_>>();
                capabilities.extend(feature.internal_tools.iter().map(|name| {
                    let effects = vec![
                        omegon_traits::RuntimeEffect::FilesystemRead,
                        omegon_traits::RuntimeEffect::FilesystemWrite,
                        omegon_traits::RuntimeEffect::ProcessSpawn,
                        omegon_traits::RuntimeEffect::NetworkAccess,
                        omegon_traits::RuntimeEffect::SecretDelivery,
                        omegon_traits::RuntimeEffect::TerminalAccess,
                        omegon_traits::RuntimeEffect::DurableStateWrite,
                        omegon_traits::RuntimeEffect::RuntimeControl,
                    ];
                    RuntimeContributionCapabilityDeclaration {
                        id: RuntimeCapabilityId::new(format!("internal:{name}"))
                            .expect("internal tool names form stable capability identifiers"),
                        kind: RuntimeCapabilityKind::KernelService,
                        service_interface: None,
                        bindings: vec![RuntimeInvocationBinding {
                            kind: omegon_traits::RuntimeInvocationKind::Internal,
                            name: name.clone(),
                            role: RuntimeInvocationBindingRole::Canonical,
                        }],
                        execution: policy(name, &effects, RuntimePrincipalClass::Internal),
                        effects,
                        transition: transition(),
                        surfaces: vec![RuntimeSurface::Internal],
                    }
                }));
                capabilities.extend(
                    feature
                        .services
                        .iter()
                        .map(|service| service.capability.clone()),
                );
                capabilities.extend(
                    retained_graph_managed_services
                        .iter()
                        .filter(|service| service.owner == feature.contribution_id)
                        .filter_map(|service| {
                            self.accepted_graph
                                .as_ref()?
                                .declarations
                                .get(&service.owner)?
                                .capabilities
                                .iter()
                                .find(|capability| capability.id == service.capability_id)
                                .cloned()
                        }),
                );
                if let Some(candidate) = managed_candidates
                    .get(&feature.name)
                    .and_then(|candidates| candidates.first())
                {
                    capabilities.extend(candidate.services().map(
                        |(capability_id, interface_id)| {
                            RuntimeContributionCapabilityDeclaration {
                                id: capability_id.clone(),
                                kind: RuntimeCapabilityKind::InProcessService,
                                service_interface: Some(interface_id.clone()),
                                bindings: Vec::new(),
                                effects: Vec::new(),
                                execution: RuntimeExecutionPolicy {
                                    principals: vec![RuntimePrincipalClass::Service],
                                    timeout_class: RuntimeTimeoutClass::Immediate,
                                    retry_class: RuntimeRetryClass::Never,
                                    idempotency: RuntimeIdempotency::NonIdempotent,
                                    deduplication: RuntimeDeduplication::Unsupported,
                                    parallelism: RuntimeParallelism::ParallelSafe,
                                    transaction: RuntimeTransactionBehavior::None,
                                    mutation_fence: None,
                                    max_attempts: Some(1),
                                },
                                transition: RuntimeCapabilityTransitionPolicy {
                                    authority_narrowing: RuntimeAuthorityNarrowing::DrainExisting,
                                    active_call_timeout_ms: u64::try_from(
                                        candidate.active_call_duration().as_millis(),
                                    )
                                    .expect("validated managed active-call timeout fits u64"),
                                },
                                surfaces: vec![RuntimeSurface::Internal],
                            }
                        },
                    ));
                }
                RuntimeContributionDeclaration {
                    schema_version: RuntimeContributionSchemaVersion::V1,
                    id: feature.contribution_id.clone(),
                    generation_id: feature.generation_id.clone(),
                    owner_tier: if external {
                        RuntimeOwnerTier::External
                    } else {
                        RuntimeOwnerTier::System
                    },
                    requested_trust: if external {
                        RuntimeTrustRequest::UntrustedDynamic
                    } else {
                        RuntimeTrustRequest::ReleaseArtifact
                    },
                    requested_confinement: RuntimeConfinementRequest::HostProcess,
                    protocol: RuntimeProtocolRange::new(1, 1)
                        .expect("static protocol range is valid"),
                    platform: RuntimePlatformRequirements::default(),
                    dependencies: feature.dependencies.clone(),
                    conflicts: vec![],
                    replaces: vec![],
                    lifecycle: feature.lifecycle.clone().unwrap_or(RuntimeLifecyclePolicy {
                        requirement: if external {
                            RuntimeLifecycleRequirement::Optional
                        } else {
                            RuntimeLifecycleRequirement::Required
                        },
                        failure_disposition: if external {
                            RuntimeFailureDisposition::DegradeLocally
                        } else {
                            RuntimeFailureDisposition::FailComposition
                        },
                        readiness_timeout_ms: 0,
                        heartbeat_timeout_ms: None,
                        restart_limit: 0,
                    }),
                    transition: feature.composition_transition.clone().unwrap_or(
                        RuntimeCompositionTransitionPolicy {
                            activation_boundary: RuntimeActivationBoundary::Boot,
                            cleanup: RuntimeCleanupRequirement::BestEffort,
                            cleanup_timeout_ms: 0,
                        },
                    ),
                    capabilities,
                    groups: vec![],
                }
            })
            .collect::<Vec<_>>();
        declarations.push(RuntimeContributionDeclaration {
            schema_version: RuntimeContributionSchemaVersion::V1,
            id: omegon_traits::RuntimeContributionId::new("system:tool-groups")
                .expect("static group owner id is valid"),
            generation_id: RuntimeContributionGenerationId::new(
                "contribution:tool-groups-static-v1",
            )
            .expect("static group generation id is valid"),
            owner_tier: RuntimeOwnerTier::System,
            requested_trust: RuntimeTrustRequest::ReleaseArtifact,
            requested_confinement: RuntimeConfinementRequest::None,
            protocol: RuntimeProtocolRange::new(1, 1).expect("static protocol range is valid"),
            platform: RuntimePlatformRequirements::default(),
            dependencies: vec![],
            conflicts: vec![],
            replaces: vec![],
            lifecycle: RuntimeLifecyclePolicy {
                requirement: RuntimeLifecycleRequirement::Required,
                failure_disposition: RuntimeFailureDisposition::FailComposition,
                readiness_timeout_ms: 0,
                heartbeat_timeout_ms: None,
                restart_limit: 0,
            },
            transition: RuntimeCompositionTransitionPolicy {
                activation_boundary: RuntimeActivationBoundary::Boot,
                cleanup: RuntimeCleanupRequirement::Strict,
                cleanup_timeout_ms: 0,
            },
            capabilities: vec![],
            groups,
        });

        let build = crate::contribution_graph::build_candidate_graph(
            crate::contribution_graph::CandidateGraphRequest {
                declarations,
                environment: crate::contribution_graph::CandidateGraphEnvironment {
                    supported_protocol: RuntimeProtocolRange::new(1, 1)
                        .expect("host protocol range is valid"),
                    operating_system: std::env::consts::OS.into(),
                    architecture: std::env::consts::ARCH.into(),
                    available_substrates: BTreeSet::from(["host".into()]),
                },
                effect_evidence: vec![],
            },
        );
        let graph = match build.graph {
            Some(graph) => graph,
            None => {
                self.composition_diagnostics = build.diagnostics.clone();
                let error = anyhow::anyhow!(
                    "candidate contribution graph rejected:\n{}",
                    serde_json::to_string_pretty(&build.diagnostics)
                        .unwrap_or_else(|_| format!("{:?}", build.diagnostics))
                );
                self.pending_features.clear();
                self.pending_internal_tools.clear();
                return Err(error);
            }
        };

        for service in &retained_graph_managed_services {
            let declaration = graph.declarations.get(&service.owner).ok_or_else(|| {
                anyhow::anyhow!(
                    "candidate graph dropped active managed-service owner {}",
                    service.owner.as_str()
                )
            })?;
            let capability = declaration
                .capabilities
                .iter()
                .find(|capability| capability.id == service.capability_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "candidate graph dropped active managed service {}",
                        service.capability_id.as_str()
                    )
                })?;
            if declaration.generation_id != service.generation_id
                || graph.capability_owners.get(&service.capability_id) != Some(&service.owner)
                || capability.kind != RuntimeCapabilityKind::InProcessService
                || capability.service_interface.as_ref() != Some(&service.interface_id)
            {
                anyhow::bail!(
                    "candidate graph changed active managed service without staging a replacement: {}",
                    service.capability_id.as_str()
                );
            }
        }
        for service in self.managed_services.direct_managed_metadata() {
            if graph.capability_owners.contains_key(&service.capability_id)
                && !staged_managed_capabilities.contains(&service.capability_id)
            {
                anyhow::bail!(
                    "candidate contribution graph collides with direct managed service {}",
                    service.capability_id.as_str()
                );
            }
        }

        let frozen_by_id = frozen
            .iter()
            .map(|feature| (feature.contribution_id.clone(), feature))
            .collect::<BTreeMap<_, _>>();
        let mut tool_defs = Vec::new();
        let mut command_defs = Vec::new();
        let mut internal_tool_owners = HashMap::new();
        let mut acp_invocation_owners = HashMap::new();
        let mut in_process_services = BTreeMap::new();
        let mut managed_service_implementations = retained_graph_managed_services.len();
        for wave in &graph.activation_waves {
            for contribution_id in wave {
                if contribution_id.as_str() != "system:tool-groups"
                    && !frozen_by_id.contains_key(contribution_id)
                {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "activation plan references an unstaged contribution: {}",
                        contribution_id.as_str()
                    );
                }
            }
        }
        for feature in &frozen {
            for definition in &feature.tools {
                let key = (
                    omegon_traits::RuntimeInvocationKind::Tool,
                    definition.name.clone(),
                );
                if graph
                    .invocation_owners
                    .get(&key)
                    .is_some_and(|(owner, _)| owner == &feature.contribution_id)
                {
                    tool_defs.push((feature.feature_index, definition.clone()));
                } else {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "staged tool is absent from the accepted graph: {}",
                        definition.name
                    );
                }
            }
            for definition in &feature.commands {
                let key = (
                    omegon_traits::RuntimeInvocationKind::Command,
                    definition.name.clone(),
                );
                if graph
                    .invocation_owners
                    .get(&key)
                    .is_some_and(|(owner, _)| owner == &feature.contribution_id)
                {
                    command_defs.push((feature.feature_index, definition.clone()));
                } else {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "staged command is absent from the accepted graph: {}",
                        definition.name
                    );
                }
            }
            for definition in &feature.acp_invocations {
                let key = (
                    omegon_traits::RuntimeInvocationKind::Acp,
                    definition.name.clone(),
                );
                if graph
                    .invocation_owners
                    .get(&key)
                    .is_some_and(|(owner, _)| owner == &feature.contribution_id)
                {
                    acp_invocation_owners.insert(definition.name.clone(), feature.feature_index);
                } else {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "staged ACP invocation is absent from the accepted graph: {}",
                        definition.name
                    );
                }
            }
            for name in &feature.internal_tools {
                let key = (omegon_traits::RuntimeInvocationKind::Internal, name.clone());
                if graph
                    .invocation_owners
                    .get(&key)
                    .is_some_and(|(owner, _)| owner == &feature.contribution_id)
                {
                    internal_tool_owners.insert(name.clone(), feature.feature_index);
                } else {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "staged internal binding is absent from the accepted graph: {name}"
                    );
                }
            }
            for service in &feature.services {
                if service.capability.kind != RuntimeCapabilityKind::InProcessService
                    || !service.capability.bindings.is_empty()
                    || service.capability.service_interface.as_ref() != Some(&service.interface_id)
                {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "in-process service {} must be bindingless and use the in_process_service capability kind",
                        service.capability.id.as_str()
                    );
                }
                if feature
                    .composition_transition
                    .as_ref()
                    .is_none_or(|transition| {
                        transition.cleanup != RuntimeCleanupRequirement::Strict
                            || transition.cleanup_timeout_ms != 0
                    })
                {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "no-resource in-process service {} requires strict zero-timeout cleanup",
                        service.capability.id.as_str()
                    );
                }
                if !graph
                    .capability_owners
                    .get(&service.capability.id)
                    .is_some_and(|owner| owner == &feature.contribution_id)
                {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "staged in-process service is absent from the accepted graph: {}",
                        service.capability.id.as_str()
                    );
                }
                let generation_id = graph
                    .declarations
                    .get(&feature.contribution_id)
                    .expect("accepted feature declaration exists")
                    .generation_id
                    .clone();
                if in_process_services
                    .insert(
                        service.capability.id.clone(),
                        PublishedInProcessService {
                            owner: feature.contribution_id.clone(),
                            generation_id,
                            interface_id: service.interface_id.clone(),
                            implementation: service.erased_implementation(),
                            implementation_identity: service.implementation_identity(),
                            implementation_type_id: service.implementation_type_id(),
                        },
                    )
                    .is_some()
                {
                    self.pending_features.clear();
                    self.pending_internal_tools.clear();
                    anyhow::bail!(
                        "multiple typed implementations were staged for in-process service {}",
                        service.capability.id.as_str()
                    );
                }
            }
            if let Some(candidate) = managed_candidates
                .get(&feature.name)
                .and_then(|candidates| candidates.first())
            {
                for (capability_id, interface_id) in candidate.services() {
                    let declaration = graph
                        .declarations
                        .get(&feature.contribution_id)
                        .expect("accepted managed owner declaration exists");
                    let graph_capability = declaration
                        .capabilities
                        .iter()
                        .find(|capability| &capability.id == capability_id)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "staged managed service is absent from the accepted graph: {}",
                                capability_id.as_str()
                            )
                        })?;
                    if graph.capability_owners.get(capability_id) != Some(&feature.contribution_id)
                        || graph_capability.kind != RuntimeCapabilityKind::InProcessService
                        || !graph_capability.bindings.is_empty()
                        || graph_capability.service_interface.as_ref() != Some(interface_id)
                    {
                        anyhow::bail!(
                            "managed service graph owner/interface parity mismatch: {}",
                            capability_id.as_str()
                        );
                    }
                    managed_service_implementations += 1;
                }
            }
        }
        let accepted_bindings = graph
            .invocation_owners
            .keys()
            .filter(|(kind, _)| {
                matches!(
                    kind,
                    omegon_traits::RuntimeInvocationKind::Tool
                        | omegon_traits::RuntimeInvocationKind::Command
                        | omegon_traits::RuntimeInvocationKind::Internal
                        | omegon_traits::RuntimeInvocationKind::Acp
                )
            })
            .count();
        let implemented_bindings = tool_defs.len()
            + command_defs.len()
            + internal_tool_owners.len()
            + acp_invocation_owners.len();
        if implemented_bindings != accepted_bindings {
            self.pending_features.clear();
            self.pending_internal_tools.clear();
            anyhow::bail!(
                "accepted graph/implementation parity mismatch: {accepted_bindings} bindings, {implemented_bindings} implementations"
            );
        }
        let accepted_services = graph
            .declarations
            .values()
            .flat_map(|declaration| &declaration.capabilities)
            .filter(|capability| capability.kind == RuntimeCapabilityKind::InProcessService)
            .count();
        let implemented_services = in_process_services.len() + managed_service_implementations;
        if implemented_services != accepted_services {
            self.pending_features.clear();
            self.pending_internal_tools.clear();
            anyhow::bail!(
                "accepted graph/service implementation parity mismatch: {accepted_services} services, {} implementations",
                implemented_services
            );
        }
        for feature in &frozen {
            let Some(active_declaration) = self
                .accepted_graph
                .as_ref()
                .and_then(|graph| graph.declarations.get(&feature.contribution_id))
            else {
                continue;
            };
            if active_declaration.generation_id != feature.generation_id {
                continue;
            }
            let active = self
                .in_process_services
                .iter()
                .filter(|(_, service)| service.owner == feature.contribution_id)
                .collect::<BTreeMap<_, _>>();
            let candidate = in_process_services
                .iter()
                .filter(|(_, service)| service.owner == feature.contribution_id)
                .collect::<BTreeMap<_, _>>();
            let unchanged = active.len() == candidate.len()
                && active.iter().all(|(capability_id, current)| {
                    candidate.get(capability_id).is_some_and(|next| {
                        current.interface_id == next.interface_id
                            && current.implementation_identity == next.implementation_identity
                            && current.implementation_type_id == next.implementation_type_id
                    })
                });
            if !unchanged {
                self.pending_features.clear();
                self.pending_internal_tools.clear();
                anyhow::bail!(
                    "in-process service contract changed without changing contribution generation: {}",
                    feature.contribution_id.as_str()
                );
            }
        }

        Ok(PreparedComposition {
            tool_defs,
            command_defs,
            internal_tool_owners,
            acp_invocation_owners,
            in_process_services,
            diagnostics: build.diagnostics,
            graph,
            generation_id,
        })
    }

    fn commit_finalization(&mut self, prepared: PreparedComposition) {
        for mutation in self.pending_features.drain(..) {
            match mutation {
                PendingFeatureMutation::Register(feature) => self.features.push(feature),
                PendingFeatureMutation::Replace(feature) => {
                    if let Some(index) = self
                        .features
                        .iter()
                        .position(|current| current.name() == feature.name())
                    {
                        self.features[index] = feature;
                    } else {
                        self.features.push(feature);
                    }
                }
            }
        }
        self.declared_internal_tools
            .append(&mut self.pending_internal_tools);
        self.tool_defs = prepared.tool_defs;
        self.command_defs = prepared.command_defs;
        self.internal_tool_owners = prepared.internal_tool_owners;
        self.acp_invocation_owners = prepared.acp_invocation_owners;
        self.in_process_services = prepared.in_process_services;
        self.composition_diagnostics = prepared.diagnostics;
        self.accepted_graph = Some(std::sync::Arc::new(prepared.graph));
        self.accepted_generation_id = Some(prepared.generation_id);
        self.published_feature_count = self.features.len();
        self.refresh_tool_inventory();
        tracing::info!(
            features = self.features.len(),
            tools = self.tool_defs.len(),
            commands = self.command_defs.len(),
            "event bus published from accepted contribution graph"
        );
    }

    fn clear_pending_ordinary(&mut self) {
        self.pending_features.clear();
        self.pending_internal_tools.clear();
    }

    /// Build the authority-neutral runtime capability inventory for the
    /// definitions currently finalized on this bus. This mirrors the legacy
    /// registries and does not participate in filtering or dispatch.
    pub fn runtime_capability_registry(&self) -> omegon_traits::RuntimeCapabilityRegistry {
        let tools = self.tool_defs.iter().map(|(feature_index, definition)| {
            crate::capability_admission::OwnedToolDefinition {
                owner: self.features[*feature_index].name().to_string(),
                definition: definition.clone(),
            }
        });
        let commands = self.command_defs.iter().map(|(feature_index, definition)| {
            crate::capability_admission::OwnedCommandDefinition {
                owner: self.features[*feature_index].name().to_string(),
                definition: definition.clone(),
            }
        });
        let mut declarations =
            crate::capability_admission::declarations_from_registries(tools, commands);
        declarations.extend(self.in_process_services.iter().map(|(id, service)| {
            omegon_traits::RuntimeCapabilityDeclaration {
                id: id.clone(),
                kind: omegon_traits::RuntimeCapabilityKind::InProcessService,
                owner: omegon_traits::RuntimeCapabilityOwner::feature(
                    service.owner.as_str().to_string(),
                ),
                invocations: Vec::new(),
            }
        }));
        declarations.extend(
            self.managed_services
                .graph_managed_metadata()
                .into_iter()
                .map(|service| omegon_traits::RuntimeCapabilityDeclaration {
                    id: service.capability_id,
                    kind: omegon_traits::RuntimeCapabilityKind::InProcessService,
                    owner: omegon_traits::RuntimeCapabilityOwner::feature(
                        service.owner.as_str().to_string(),
                    ),
                    invocations: Vec::new(),
                }),
        );
        let groups = crate::features::manage_tools::TOOL_GROUPS
            .iter()
            .map(|(name, members)| omegon_traits::RuntimeCapabilityGroup {
                name: (*name).to_string(),
                members: members
                    .iter()
                    .map(|member| omegon_traits::RuntimeCapabilityId::tool(member))
                    .collect(),
            })
            .collect();
        crate::capability_admission::validate_registry(declarations, groups)
    }

    fn refresh_tool_inventory(&self) {
        let Some(handle) = &self.tool_inventory else {
            return;
        };
        let mut registered: Vec<String> = self
            .tool_defs
            .iter()
            .filter(|(_, def)| !is_model_hidden_tool(&def.name))
            .map(|(_, def)| def.name.clone())
            .collect();
        registered.sort();

        let mut callable: Vec<String> = self
            .tool_definitions_mode(false)
            .into_iter()
            .map(|def| def.name)
            .collect();
        callable.sort();

        if let Ok(mut inventory) = handle.lock() {
            *inventory = crate::features::manage_tools::ToolInventorySnapshot {
                registered,
                callable,
            };
        }
    }

    /// Visible tool names captured by ManageTools, ignoring disabled state.
    #[cfg(test)]
    fn tool_inventory_names(&self) -> Vec<String> {
        self.tool_inventory
            .as_ref()
            .and_then(|handle| {
                handle
                    .lock()
                    .ok()
                    .map(|snapshot| snapshot.registered.clone())
            })
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn callable_tool_inventory_names(&self) -> Vec<String> {
        self.tool_inventory
            .as_ref()
            .and_then(|handle| handle.lock().ok().map(|snapshot| snapshot.callable.clone()))
            .unwrap_or_default()
    }

    // ─── Event delivery ─────────────────────────────────────────────

    /// Deliver an event to all features. Requests are accumulated
    /// and can be drained with `drain_requests()`.
    pub fn emit(&mut self, event: &BusEvent) {
        for feature in &mut self.features[..self.published_feature_count] {
            let requests = feature.on_event(event);
            self.pending_requests.extend(requests);
        }
    }

    /// Drain accumulated requests from the most recent event deliveries.
    pub fn drain_requests(&mut self) -> Vec<BusRequest> {
        std::mem::take(&mut self.pending_requests)
    }

    /// Emit a HarnessStatusChanged event from an updated status snapshot.
    /// Also returns the serialized JSON for forwarding to AgentEvent broadcast.
    pub fn emit_harness_status(&mut self, status: &crate::status::HarnessStatus) -> Value {
        let status_json = serde_json::to_value(status).unwrap_or_default();
        self.emit(&BusEvent::HarnessStatusChanged {
            status_json: status_json.clone(),
        });
        status_json
    }

    // ─── Tool dispatch ──────────────────────────────────────────────

    /// All tool definitions across all features.
    /// When `compact` is true, strips parameter descriptions from JSON schemas
    /// to reduce token overhead (~30-40% savings on tool schema tokens).
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_definitions_mode(false)
    }

    pub(crate) fn callable_tool_definitions_by_owner(&self) -> Vec<(String, ToolDefinition)> {
        let disabled = self
            .tool_admission
            .as_ref()
            .and_then(|policy| policy.lock().ok());
        self.tool_defs
            .iter()
            .filter(|(_, definition)| {
                disabled
                    .as_ref()
                    .is_none_or(|policy| !policy.contains(&definition.name))
            })
            .filter(|(_, definition)| !self.is_tool_denied_by_policy(&definition.name))
            .filter(|(_, definition)| !is_model_hidden_tool(&definition.name))
            .map(|(index, definition)| {
                (self.features[*index].name().to_string(), definition.clone())
            })
            .collect()
    }

    pub fn has_tool(&self, tool_name: &str) -> bool {
        self.tool_defs.iter().any(|(_, def)| def.name == tool_name)
            || self.internal_tool_owners.contains_key(tool_name)
    }

    pub(crate) fn resolve_invocation(
        &self,
        kind: omegon_traits::RuntimeInvocationKind,
        name: &str,
    ) -> Result<
        crate::invocation_service::ResolvedInvocation,
        crate::invocation_service::InvocationDenial,
    > {
        use crate::invocation_service::{InvocationDenialCode, ResolvedInvocation, denial};

        let graph = self.accepted_graph.as_ref().ok_or_else(|| {
            denial(
                InvocationDenialCode::IncompleteDeclaration,
                "no accepted contribution graph is published",
            )
        })?;
        let composition_generation_id = self.accepted_generation_id.clone().ok_or_else(|| {
            denial(
                InvocationDenialCode::IncompleteDeclaration,
                "accepted graph has no composition generation",
            )
        })?;
        let (contribution_id, capability_id) = graph
            .invocation_owners
            .get(&(kind, name.to_string()))
            .cloned()
            .ok_or_else(|| {
                denial(
                    InvocationDenialCode::UnknownInvocation,
                    format!("no accepted capability owns {kind:?} invocation {name:?}"),
                )
            })?;
        let declaration = graph.declarations.get(&contribution_id).ok_or_else(|| {
            denial(
                InvocationDenialCode::IncompleteDeclaration,
                "accepted invocation owner has no contribution declaration",
            )
        })?;
        let capability = declaration
            .capabilities
            .iter()
            .find(|capability| capability.id == capability_id)
            .ok_or_else(|| {
                denial(
                    InvocationDenialCode::IncompleteDeclaration,
                    "accepted invocation owner has no capability declaration",
                )
            })?;
        if !capability
            .bindings
            .iter()
            .any(|binding| binding.kind == kind && binding.name == name)
        {
            return Err(denial(
                InvocationDenialCode::IncompleteDeclaration,
                "capability declaration does not contain the accepted invocation binding",
            ));
        }

        Ok(ResolvedInvocation {
            kind,
            name: name.to_string(),
            capability_id,
            contribution_id,
            owner_generation_id: declaration.generation_id.clone(),
            composition_generation_id,
            effects: capability.effects.clone(),
            execution: capability.execution.clone(),
            transition: capability.transition.clone(),
            surfaces: capability.surfaces.clone(),
        })
    }

    pub(crate) fn validate_execution_lease(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        call_id: &str,
        kind: omegon_traits::RuntimeInvocationKind,
        invocation_name: &str,
    ) -> Result<(), crate::invocation_service::InvocationDenial> {
        use crate::invocation_service::{InvocationDenialCode, LeaseTerminal, denial};

        if lease.terminal() != LeaseTerminal::Dispatching {
            return Err(denial(
                InvocationDenialCode::LeaseClosed,
                "execution lease has not been claimed or is already terminal",
            ));
        }
        if lease.call_id != call_id
            || lease.invocation_name != invocation_name
            || lease.kind != kind
        {
            lease.revoke();
            return Err(denial(
                InvocationDenialCode::LeaseMismatch,
                "execution lease does not match the dispatch request",
            ));
        }
        if self.accepted_generation_id.as_ref() != Some(&lease.issue_generation_id) {
            lease.revoke();
            return Err(denial(
                InvocationDenialCode::StaleGeneration,
                "execution lease was issued for a stale composition generation",
            ));
        }
        let current = self.resolve_invocation(kind, invocation_name)?;
        if current.capability_id != lease.capability_id
            || current.contribution_id != lease.contribution_id
            || current.owner_generation_id != lease.owner_generation_id
            || current.effects != lease.admitted_effects
            || current.execution != lease.execution
            || current.transition != lease.transition
            || current.surfaces != lease.surfaces
        {
            lease.revoke();
            return Err(denial(
                InvocationDenialCode::StaleGeneration,
                "accepted invocation owner no longer matches the execution lease",
            ));
        }
        Ok(())
    }

    /// Authoritative producer from the validated invocation-owner graph.
    pub fn tool_provenance(&self, tool_name: &str) -> omegon_traits::ToolProvenance {
        let owner = self
            .tool_defs
            .iter()
            .find(|(_, def)| def.name == tool_name)
            .map(|(idx, _)| *idx)
            .or_else(|| self.internal_tool_owners.get(tool_name).copied());
        owner
            .and_then(|idx| self.features.get(idx))
            .map(|feature| feature.tool_provenance())
            .unwrap_or_default()
    }

    /// Tool definitions with optional schema compaction for token efficiency.
    pub fn tool_definitions_mode(&self, compact: bool) -> Vec<ToolDefinition> {
        let disabled = self.tool_admission.as_ref().and_then(|d| d.lock().ok());
        self.tool_defs
            .iter()
            .filter(|(_, d)| disabled.as_ref().is_none_or(|set| !set.contains(&d.name)))
            .filter(|(_, d)| !self.is_tool_denied_by_policy(&d.name))
            .filter(|(_, d)| !is_model_hidden_tool(&d.name))
            .map(|(_, d)| {
                if compact {
                    compact_tool_schema(d)
                } else {
                    d.clone()
                }
            })
            .collect()
    }

    /// Lazy tool injection: returns a reduced tool set for token efficiency.
    ///
    /// - **Turn 1**: all enabled tools (model needs to see the full surface once)
    /// - **Turn 2+**: core tools always + extended tools only if previously used
    ///   or contextually relevant
    ///
    /// `used_tools` is the set of tool names called so far in this session.
    pub fn tool_definitions_lazy(
        &self,
        compact: bool,
        turn: u32,
        used_tools: &std::collections::HashSet<String>,
    ) -> Vec<ToolDefinition> {
        self.tool_definitions_lazy_inner(compact, turn, used_tools, false)
    }

    /// Like `tool_definitions_lazy` but with `core_only_turn1` = true, the
    /// first turn also only receives core tools. Use for constrained models
    /// (≤32B) where 50+ tool schemas overwhelm the context window.
    pub fn tool_definitions_lean(
        &self,
        turn: u32,
        used_tools: &std::collections::HashSet<String>,
    ) -> Vec<ToolDefinition> {
        self.tool_definitions_lazy_inner(true, turn, used_tools, true)
    }

    fn tool_definitions_lazy_inner(
        &self,
        compact: bool,
        turn: u32,
        used_tools: &std::collections::HashSet<String>,
        core_only_turn1: bool,
    ) -> Vec<ToolDefinition> {
        // Turn 1: full surface so the model knows what's available —
        // unless core_only_turn1 is set (constrained models with small ctx).
        if turn <= 1 && !core_only_turn1 {
            return self.tool_definitions_mode(compact);
        }

        let disabled = self.tool_admission.as_ref().and_then(|d| d.lock().ok());
        self.tool_defs
            .iter()
            .filter(|(_, d)| disabled.as_ref().is_none_or(|set| !set.contains(&d.name)))
            .filter(|(_, d)| !self.is_tool_denied_by_policy(&d.name))
            .filter(|(_, d)| !is_model_hidden_tool(&d.name))
            .filter(|(_, d)| {
                is_core_tool(&d.name) || is_dynamic_tool(&d.name) || used_tools.contains(&d.name)
            })
            .map(|(_, d)| {
                if compact {
                    compact_tool_schema(d)
                } else {
                    d.clone()
                }
            })
            .collect()
    }

    /// All registered tool definitions, ignoring disabled state.
    /// Used for the manage_tools list command.
    #[allow(dead_code)]
    pub fn all_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_defs
            .iter()
            .filter(|(_, d)| !is_model_hidden_tool(&d.name))
            .map(|(_, d)| d.clone())
            .collect()
    }

    /// Returns true when a tool is registered in the runtime, including
    /// model-hidden tools and explicit internal tool owners.
    pub fn has_registered_tool(&self, tool_name: &str) -> bool {
        self.tool_defs.iter().any(|(_, d)| d.name == tool_name)
            || self.internal_tool_owners.contains_key(tool_name)
    }

    /// Find which feature owns a tool and execute it.
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        self.execute_tool_with_sink(
            tool_name,
            call_id,
            args,
            cancel,
            omegon_traits::ToolProgressSink::noop(),
        )
        .await
    }

    /// Like [`Self::execute_tool`] but also passes a `ToolProgressSink` so the
    /// runner can stream partial output. The dispatch loop in `loop.rs` uses
    /// this path; other call sites that just want a final result keep using
    /// [`Self::execute_tool`] (which constructs a no-op sink).
    pub async fn execute_tool_with_sink(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        sink: omegon_traits::ToolProgressSink,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        let default_timeout = self
            .tool_timeouts
            .get(tool_name)
            .copied()
            .unwrap_or(DEFAULT_TOOL_TIMEOUT);

        let timeout = effective_tool_timeout(tool_name, &args, default_timeout);

        for (idx, def) in &self.tool_defs {
            if def.name == tool_name {
                let execution = self.features[*idx].execute_with_context(
                    tool_name,
                    call_id,
                    args,
                    cancel,
                    sink,
                    omegon_traits::ToolExecutionContext::default(),
                );
                let Some(timeout) = timeout else {
                    return execution.await;
                };
                return match tokio::time::timeout(timeout, execution).await {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        tracing::error!(
                            tool = tool_name,
                            timeout_secs = timeout.as_secs(),
                            "tool execution timed out"
                        );
                        Ok(omegon_traits::ToolResult {
                            content: vec![omegon_traits::ContentBlock::Text {
                                text: format!(
                                    "Tool '{}' timed out after {} seconds. \
                                     The operation was cancelled.",
                                    tool_name,
                                    timeout.as_secs()
                                ),
                            }],
                            details: serde_json::json!({"is_error": true}),
                        })
                    }
                };
            }
        }
        anyhow::bail!("no feature provides tool '{tool_name}'")
    }

    /// Execute a tool with a host interaction context. Used by ACP-hosted
    /// sessions to route operator approval requests back to the client.
    pub async fn execute_tool_with_context(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        sink: omegon_traits::ToolProgressSink,
        context: omegon_traits::ToolExecutionContext,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        let default_timeout = self
            .tool_timeouts
            .get(tool_name)
            .copied()
            .unwrap_or(DEFAULT_TOOL_TIMEOUT);
        let timeout = effective_tool_timeout(tool_name, &args, default_timeout);

        for (idx, def) in &self.tool_defs {
            if def.name == tool_name {
                let execution = self.features[*idx]
                    .execute_with_context(tool_name, call_id, args, cancel, sink, context);
                let Some(timeout) = timeout else {
                    return execution.await;
                };
                return match tokio::time::timeout(timeout, execution).await {
                    Ok(result) => result,
                    Err(_elapsed) => Ok(omegon_traits::ToolResult {
                        content: vec![omegon_traits::ContentBlock::Text {
                            text: format!(
                                "Tool '{}' timed out after {} seconds. The operation was cancelled.",
                                tool_name,
                                timeout.as_secs()
                            ),
                        }],
                        details: serde_json::json!({"is_error": true}),
                    }),
                };
            }
        }
        anyhow::bail!("no feature provides tool '{tool_name}'")
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_tool_with_lease(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        sink: omegon_traits::ToolProgressSink,
        mut context: omegon_traits::ToolExecutionContext,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        self.validate_execution_lease(
            lease,
            call_id,
            omegon_traits::RuntimeInvocationKind::Tool,
            tool_name,
        )
        .map_err(|denial| anyhow::anyhow!("{}: {}", denial.code.as_str(), denial.message))?;
        context.invocation = Some(lease.dispatch_metadata());
        context.host_action_invocation = Some(lease.host_action_guard());
        let timeout = lease.execution_timeout(&args);
        for (idx, def) in &self.tool_defs {
            if def.name == tool_name {
                let execution_cancel = cancel.child_token();
                let execution = self.features[*idx].execute_with_invocation_control(
                    tool_name,
                    call_id,
                    args,
                    execution_cancel.clone(),
                    sink,
                    context,
                    lease.invocation_control(),
                );
                return match tokio::time::timeout(timeout, execution).await {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        execution_cancel.cancel();
                        Ok(omegon_traits::ToolResult {
                            content: vec![omegon_traits::ContentBlock::Text {
                                text: format!(
                                    "Tool '{}' timed out after {} seconds. The operation was cancelled.",
                                    tool_name,
                                    timeout.as_secs()
                                ),
                            }],
                            details: serde_json::json!({"is_error": true}),
                        })
                    }
                };
            }
        }
        anyhow::bail!("no feature provides tool '{tool_name}'")
    }

    pub(crate) async fn invoke_tool(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        scope: crate::invocation_service::InvocationScope,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        let admission = crate::invocation_service::InvocationService::admit_invocation(
            self,
            omegon_traits::RuntimeInvocationKind::Tool,
            tool_name,
            crate::invocation_service::InvocationRequest {
                call_id,
                scope,
                permission_policy: None,
                permission_role: None,
                permission_name: tool_name,
                permission_subjects: &[],
            },
        );
        let lease = match admission {
            crate::invocation_service::InvocationAdmission::Lease(lease) => lease,
            crate::invocation_service::InvocationAdmission::Denied(denial) => {
                anyhow::bail!("{}: {}", denial.code.as_str(), denial.message)
            }
            crate::invocation_service::InvocationAdmission::ApprovalRequired(_) => {
                anyhow::bail!("invocation:approval_denied: tool invocation requires approval")
            }
        };
        lease
            .claim_dispatch(call_id, tool_name)
            .and_then(|_| {
                self.validate_execution_lease(
                    &lease,
                    call_id,
                    omegon_traits::RuntimeInvocationKind::Tool,
                    tool_name,
                )
            })
            .and_then(|_| lease.persist_dispatched())
            .map_err(|denial| anyhow::anyhow!("{}: {}", denial.code.as_str(), denial.message))?;

        let result = self
            .execute_tool_with_lease(
                &lease,
                tool_name,
                call_id,
                args,
                cancel,
                omegon_traits::ToolProgressSink::noop(),
                omegon_traits::ToolExecutionContext::default(),
            )
            .await;
        let is_error = match &result {
            Err(_) => true,
            Ok(result) => result
                .details
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        let (outcome, terminal) = if is_error {
            (
                crate::session_authority::InvocationOutcome::Failed,
                crate::invocation_service::LeaseTerminal::Failed,
            )
        } else {
            (
                crate::session_authority::InvocationOutcome::Completed,
                crate::invocation_service::LeaseTerminal::Completed,
            )
        };
        lease
            .persist_settlement(outcome)
            .map_err(|denial| anyhow::anyhow!("{}: {}", denial.code.as_str(), denial.message))?;
        if !lease.close(terminal) {
            anyhow::bail!("invocation:lease_closed: tool execution lease was already closed");
        }
        result
    }

    /// Execute an internal tool that may not be in the LLM-visible tool_defs.
    ///
    /// Execute an internal tool that may not be in the LLM-visible tool_defs.
    ///
    /// Unlike `execute_tool`, this doesn't require the tool to be in the
    /// registered definitions. It uses the `internal_tool_owners` map
    /// populated at registration time to find the owning feature directly.
    pub async fn execute_internal(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        // Check the internal tool owner map first
        if let Some(&idx) = self.internal_tool_owners.get(tool_name) {
            return self.features[idx]
                .execute(tool_name, call_id, args, cancel)
                .await;
        }
        // Fallback: check tool_defs (tool might be registered but disabled)
        for (idx, def) in &self.tool_defs {
            if def.name == tool_name {
                return self.features[*idx]
                    .execute(tool_name, call_id, args, cancel)
                    .await;
            }
        }
        anyhow::bail!("no feature handles internal tool '{tool_name}'")
    }

    pub(crate) async fn execute_internal_with_lease(
        &self,
        lease: &crate::invocation_service::ExecutionLease,
        name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        lease
            .claim_dispatch(call_id, name)
            .and_then(|_| {
                self.validate_execution_lease(
                    lease,
                    call_id,
                    omegon_traits::RuntimeInvocationKind::Internal,
                    name,
                )
            })
            .and_then(|_| lease.persist_dispatched())
            .map_err(|denial| anyhow::anyhow!("{}: {}", denial.code.as_str(), denial.message))?;

        let idx = self
            .internal_tool_owners
            .get(name)
            .copied()
            .or_else(|| {
                self.tool_defs
                    .iter()
                    .find(|(_, definition)| definition.name == name)
                    .map(|(idx, _)| *idx)
            })
            .ok_or_else(|| anyhow::anyhow!("no feature handles internal tool '{name}'"))?;
        lease
            .invocation_control()
            .acknowledge()
            .map_err(anyhow::Error::msg)?;
        let result = self.features[idx]
            .execute(name, call_id, args, cancel)
            .await;
        let (outcome, terminal) = if result.is_ok() {
            (
                crate::session_authority::InvocationOutcome::Completed,
                crate::invocation_service::LeaseTerminal::Completed,
            )
        } else {
            (
                crate::session_authority::InvocationOutcome::Failed,
                crate::invocation_service::LeaseTerminal::Failed,
            )
        };
        lease
            .persist_settlement(outcome)
            .map_err(|denial| anyhow::anyhow!("{}: {}", denial.code.as_str(), denial.message))?;
        if !lease.close(terminal) {
            anyhow::bail!("invocation:lease_closed: internal execution lease was already closed");
        }
        result
    }

    pub(crate) async fn invoke_internal(
        &self,
        name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        scope: crate::invocation_service::InvocationScope,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        let admission = crate::invocation_service::InvocationService::admit_invocation(
            self,
            omegon_traits::RuntimeInvocationKind::Internal,
            name,
            crate::invocation_service::InvocationRequest {
                call_id,
                scope,
                permission_policy: None,
                permission_role: None,
                permission_name: name,
                permission_subjects: &[],
            },
        );
        let lease = match admission {
            crate::invocation_service::InvocationAdmission::Lease(lease) => lease,
            crate::invocation_service::InvocationAdmission::Denied(denial) => {
                anyhow::bail!("{}: {}", denial.code.as_str(), denial.message)
            }
            crate::invocation_service::InvocationAdmission::ApprovalRequired(_) => {
                anyhow::bail!("invocation:approval_denied: internal invocation requires approval")
            }
        };
        self.execute_internal_with_lease(&lease, name, call_id, args, cancel)
            .await
    }

    pub(crate) async fn invoke_acp(
        &self,
        name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        scope: crate::invocation_service::InvocationScope,
    ) -> anyhow::Result<Value> {
        let admission = crate::invocation_service::InvocationService::admit_invocation(
            self,
            omegon_traits::RuntimeInvocationKind::Acp,
            name,
            crate::invocation_service::InvocationRequest {
                call_id,
                scope,
                permission_policy: None,
                permission_role: None,
                permission_name: name,
                permission_subjects: &[],
            },
        );
        let lease = match admission {
            crate::invocation_service::InvocationAdmission::Lease(lease) => lease,
            crate::invocation_service::InvocationAdmission::Denied(denial) => {
                anyhow::bail!("{}: {}", denial.code.as_str(), denial.message)
            }
            crate::invocation_service::InvocationAdmission::ApprovalRequired(_) => {
                anyhow::bail!("invocation:approval_denied: ACP invocation requires approval")
            }
        };
        lease
            .claim_dispatch(call_id, name)
            .and_then(|_| {
                self.validate_execution_lease(
                    &lease,
                    call_id,
                    omegon_traits::RuntimeInvocationKind::Acp,
                    name,
                )
            })
            .and_then(|_| lease.persist_dispatched())
            .map_err(|denial| anyhow::anyhow!("{}: {}", denial.code.as_str(), denial.message))?;
        let owner = self
            .acp_invocation_owners
            .get(name)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("no feature handles ACP invocation '{name}'"))?;
        lease
            .invocation_control()
            .acknowledge()
            .map_err(anyhow::Error::msg)?;
        let result = self.features[owner]
            .execute_acp_invocation(name, args, cancel)
            .await;
        if let Err(error) = &result
            && error
                .downcast_ref::<crate::invocation_service::UnknownCompletionError>()
                .is_some()
        {
            lease
                .persist_unknown("owner_completion_unknown")
                .map_err(|denial| {
                    anyhow::anyhow!("{}: {}", denial.code.as_str(), denial.message)
                })?;
            lease.revoke();
            return result;
        }
        let (outcome, terminal) = if result.is_ok() {
            (
                crate::session_authority::InvocationOutcome::Completed,
                crate::invocation_service::LeaseTerminal::Completed,
            )
        } else {
            (
                crate::session_authority::InvocationOutcome::Failed,
                crate::invocation_service::LeaseTerminal::Failed,
            )
        };
        lease
            .persist_settlement(outcome)
            .map_err(|denial| anyhow::anyhow!("{}: {}", denial.code.as_str(), denial.message))?;
        if !lease.close(terminal) {
            anyhow::bail!("invocation:lease_closed: ACP invocation lease was already closed");
        }
        result
    }

    /// Get the configured timeout for a tool.
    pub fn tool_timeout(&self, tool_name: &str) -> Duration {
        self.tool_timeouts
            .get(tool_name)
            .copied()
            .unwrap_or(DEFAULT_TOOL_TIMEOUT)
    }

    // ─── Context injection ──────────────────────────────────────────

    /// Collect context injections from all features.
    pub fn collect_context(&self, signals: &ContextSignals<'_>) -> Vec<ContextInjection> {
        self.features
            .iter()
            .take(self.published_feature_count)
            .filter_map(|f| f.provide_context(signals))
            .collect()
    }

    // ─── Command dispatch ───────────────────────────────────────────

    /// All registered command definitions (for the command palette).
    pub fn command_definitions(&self) -> &[(usize, CommandDefinition)] {
        &self.command_defs
    }

    /// Dispatch a slash command to the feature that owns it.
    /// Returns the result from the first feature that handles it.
    pub fn dispatch_command(&mut self, name: &str, args: &str) -> CommandResult {
        // Find features that registered this command and try them
        let owning_indices: Vec<usize> = self
            .command_defs
            .iter()
            .filter(|(_, def)| def.name == name)
            .map(|(idx, _)| *idx)
            .collect();

        for idx in owning_indices {
            let result = self.features[idx].handle_command(name, args);
            if !matches!(result, CommandResult::NotHandled) {
                return result;
            }
        }
        CommandResult::NotHandled
    }

    pub(crate) fn dispatch_command_with_lease(
        &mut self,
        lease: &crate::invocation_service::ExecutionLease,
        name: &str,
        call_id: &str,
        args: &str,
    ) -> Result<CommandResult, crate::invocation_service::InvocationDenial> {
        use crate::invocation_service::{InvocationDenialCode, LeaseTerminal, denial};

        lease.claim_dispatch(call_id, name)?;
        self.validate_execution_lease(
            lease,
            call_id,
            omegon_traits::RuntimeInvocationKind::Command,
            name,
        )?;
        lease.persist_dispatched()?;
        lease.invocation_control().acknowledge().map_err(|error| {
            denial(
                InvocationDenialCode::AuthorityUnavailable,
                format!("failed to persist command acknowledgement: {error}"),
            )
        })?;

        let result = self.dispatch_command(name, args);
        let (outcome, terminal) = if matches!(result, CommandResult::NotHandled) {
            (
                crate::session_authority::InvocationOutcome::Failed,
                LeaseTerminal::Failed,
            )
        } else {
            (
                crate::session_authority::InvocationOutcome::Completed,
                LeaseTerminal::Completed,
            )
        };
        lease.persist_settlement(outcome)?;
        if !lease.close(terminal) {
            return Err(denial(
                InvocationDenialCode::LeaseClosed,
                "command execution lease was already closed",
            ));
        }
        Ok(result)
    }

    pub(crate) fn invoke_command(
        &mut self,
        name: &str,
        call_id: &str,
        args: &str,
        scope: crate::invocation_service::InvocationScope,
        permission_role: Option<styrene_rbac::Role>,
    ) -> Result<CommandResult, crate::invocation_service::InvocationDenial> {
        let admission = crate::invocation_service::InvocationService::admit_invocation(
            self,
            omegon_traits::RuntimeInvocationKind::Command,
            name,
            crate::invocation_service::InvocationRequest {
                call_id,
                scope,
                permission_policy: None,
                permission_role,
                permission_name: name,
                permission_subjects: &[],
            },
        );
        let lease = match admission {
            crate::invocation_service::InvocationAdmission::Lease(lease) => lease,
            crate::invocation_service::InvocationAdmission::Denied(denial) => return Err(denial),
            crate::invocation_service::InvocationAdmission::ApprovalRequired(_) => {
                return Err(crate::invocation_service::denial(
                    crate::invocation_service::InvocationDenialCode::ApprovalDenied,
                    "command invocation requires approval before dispatch",
                ));
            }
        };
        self.dispatch_command_with_lease(&lease, name, call_id, args)
    }

    // ─── Introspection ──────────────────────────────────────────────

    /// Number of registered features.
    pub fn feature_count(&self) -> usize {
        self.published_feature_count
    }

    /// Feature names for logging/debugging.
    pub fn feature_names(&self) -> Vec<&str> {
        self.features
            .iter()
            .take(self.published_feature_count)
            .map(|f| f.name())
            .collect()
    }
}

impl Drop for EventBus {
    fn drop(&mut self) {
        if self.managed_services.requires_ownership_retention()
            && let Some(retention) = &self.runtime_ownership_retention
        {
            retention.store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use async_trait::async_trait;
    use omegon_traits::{
        ContentBlock, Feature, ManagedCallContext, ManagedResourceController,
        ManagedResourceSettlementFuture, ManagedServiceContract, ManagedServiceFuture,
        RuntimeActivationBoundary, RuntimeCleanupAssurance, RuntimeCleanupRequirement,
        RuntimeCleanupState, RuntimeCompositionTransitionPolicy, RuntimeContributionLifecycleState,
        RuntimeFailureDisposition, RuntimeLifecyclePolicy, RuntimeLifecycleRequirement,
        ToolDefinition, ToolResult,
    };
    use serde_json::json;

    struct FailingShutdownFeature;

    #[async_trait]
    impl Feature for FailingShutdownFeature {
        fn name(&self) -> &str {
            "failing-shutdown"
        }

        async fn prepare_managed_shutdown(&mut self) -> anyhow::Result<()> {
            anyhow::bail!("owned task failed")
        }
    }

    #[tokio::test]
    async fn feature_task_failure_makes_strict_managed_shutdown_fail() {
        let mut bus = EventBus::new();
        bus.register(Box::new(FailingShutdownFeature));
        let report = bus.shutdown_managed_services().await;
        assert!(!report.all_resources_settled());
        assert_eq!(
            report.feature_failures,
            vec!["failing-shutdown: owned task failed"]
        );
        let mut bus = EventBus::new();
        bus.register(Box::new(FailingShutdownFeature));
        assert!(bus.shutdown_managed_services_strict().await.is_err());
    }

    /// Test feature that counts events and provides a tool.
    struct CounterFeature {
        event_count: u32,
    }

    #[async_trait]
    impl Feature for CounterFeature {
        fn name(&self) -> &str {
            "counter"
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "count".into(),
                label: "count".into(),
                description: "Returns the event count".into(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![],
            }]
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("count: {}", self.event_count),
                }],
                details: json!(null),
            })
        }

        fn on_event(&mut self, _event: &BusEvent) -> Vec<BusRequest> {
            self.event_count += 1;
            vec![]
        }
    }

    /// Feature that emits requests on specific events.
    struct NotifierFeature;

    #[async_trait]
    impl Feature for NotifierFeature {
        fn name(&self) -> &str {
            "notifier"
        }

        fn commands(&self) -> Vec<CommandDefinition> {
            vec![CommandDefinition {
                name: "notify".into(),
                description: "Send a test notification".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::READ_ONLY,
                surface: Default::default(),
            }]
        }

        fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
            if name == "notify" {
                CommandResult::Display(format!("Notified: {args}"))
            } else {
                CommandResult::NotHandled
            }
        }

        fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
            if matches!(event, BusEvent::SessionEnd { .. }) {
                vec![BusRequest::Notify {
                    message: "Session ended".into(),
                    level: omegon_traits::NotifyLevel::Info,
                }]
            } else {
                vec![]
            }
        }
    }

    struct ExtensionCounterFeature;

    #[async_trait]
    impl Feature for ExtensionCounterFeature {
        fn name(&self) -> &str {
            "recro-coe-agent"
        }

        fn tool_provenance(&self) -> omegon_traits::ToolProvenance {
            omegon_traits::ToolProvenance::Extension {
                name: self.name().into(),
            }
        }

        fn runtime_lifecycle_policy(&self) -> Option<RuntimeLifecyclePolicy> {
            Some(RuntimeLifecyclePolicy {
                requirement: RuntimeLifecycleRequirement::Optional,
                failure_disposition: RuntimeFailureDisposition::Quarantine,
                readiness_timeout_ms: 2_500,
                heartbeat_timeout_ms: Some(10_000),
                restart_limit: 3,
            })
        }

        fn runtime_transition_policy(&self) -> Option<RuntimeCompositionTransitionPolicy> {
            Some(RuntimeCompositionTransitionPolicy {
                activation_boundary: RuntimeActivationBoundary::Boot,
                cleanup: RuntimeCleanupRequirement::Strict,
                cleanup_timeout_ms: 500,
            })
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "count".into(),
                label: "count".into(),
                description: "Extension override".into(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![],
            }]
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: "extension".into(),
                }],
                details: json!(null),
            })
        }
    }

    struct DisplayNameFeature(&'static str);

    #[async_trait]
    impl Feature for DisplayNameFeature {
        fn name(&self) -> &str {
            self.0
        }
    }

    struct PanicToolFeature(&'static str);

    #[async_trait]
    impl Feature for PanicToolFeature {
        fn name(&self) -> &str {
            "panic-owner"
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: self.0.into(),
                label: self.0.into(),
                description: "must not execute in stale lease tests".into(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![],
            }]
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            panic!("stale lease reached its owner")
        }
    }

    struct DeclaredPolicyFeature {
        feature_name: &'static str,
        tool_name: &'static str,
        effects: Vec<omegon_traits::RuntimeEffect>,
        principals: Vec<omegon_traits::RuntimePrincipalClass>,
    }

    struct ServiceToolFeature;

    struct AcpInvocationFeature;

    trait ReadOnlyTestServiceContract: std::any::Any + Send + Sync {
        fn value(&self) -> u64;
    }

    #[derive(Debug)]
    struct ReadOnlyTestService {
        value: u64,
    }

    impl ReadOnlyTestServiceContract for ReadOnlyTestService {
        fn value(&self) -> u64 {
            self.value
        }
    }

    struct InProcessServiceFeature {
        generation: &'static str,
        interface: &'static str,
        service: std::sync::Arc<dyn ReadOnlyTestServiceContract>,
        publish: bool,
        publish_additional: bool,
    }

    struct MalformedInProcessServiceFeature;

    struct SyntheticManagedFeature {
        generation: &'static str,
        boundary: RuntimeActivationBoundary,
        cleanup_timeout_ms: u64,
    }

    struct SyntheticBestEffortManagedFeature;

    #[async_trait]
    impl Feature for SyntheticManagedFeature {
        fn name(&self) -> &str {
            "synthetic-managed"
        }

        fn runtime_contribution_generation_id(
            &self,
        ) -> Option<omegon_traits::RuntimeContributionGenerationId> {
            Some(
                omegon_traits::RuntimeContributionGenerationId::new(self.generation)
                    .expect("synthetic generation is valid"),
            )
        }

        fn runtime_transition_policy(&self) -> Option<RuntimeCompositionTransitionPolicy> {
            Some(RuntimeCompositionTransitionPolicy {
                activation_boundary: self.boundary,
                cleanup: RuntimeCleanupRequirement::Strict,
                cleanup_timeout_ms: self.cleanup_timeout_ms,
            })
        }
    }

    #[async_trait]
    impl Feature for SyntheticBestEffortManagedFeature {
        fn name(&self) -> &str {
            "synthetic-managed"
        }

        fn runtime_contribution_generation_id(
            &self,
        ) -> Option<omegon_traits::RuntimeContributionGenerationId> {
            Some(
                omegon_traits::RuntimeContributionGenerationId::new(
                    "contribution:synthetic-managed-v1",
                )
                .unwrap(),
            )
        }

        fn runtime_transition_policy(&self) -> Option<RuntimeCompositionTransitionPolicy> {
            Some(RuntimeCompositionTransitionPolicy {
                activation_boundary: RuntimeActivationBoundary::Boot,
                cleanup: RuntimeCleanupRequirement::BestEffort,
                cleanup_timeout_ms: 10,
            })
        }
    }

    struct SyntheticManagedService(usize);

    impl ManagedServiceContract for SyntheticManagedService {
        type Request = ();
        type Response = usize;
        type Error = String;

        fn execute<'a>(
            &'a self,
            (): Self::Request,
            _context: ManagedCallContext,
        ) -> ManagedServiceFuture<'a, Self::Response, Self::Error> {
            Box::pin(async move { Ok(self.0) })
        }
    }

    struct SyntheticManagedResource {
        stops: AtomicUsize,
        settled: AtomicBool,
        changed: tokio::sync::Notify,
    }

    impl SyntheticManagedResource {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                stops: AtomicUsize::new(0),
                settled: AtomicBool::new(false),
                changed: tokio::sync::Notify::new(),
            })
        }
    }

    impl ManagedResourceController for SyntheticManagedResource {
        fn request_stop(&self) {
            self.stops.fetch_add(1, Ordering::AcqRel);
            self.settled.store(true, Ordering::Release);
            self.changed.notify_waiters();
        }

        fn force_stop(&self) {}

        fn await_settled(&self) -> ManagedResourceSettlementFuture<'_> {
            Box::pin(async move {
                while !self.settled.load(Ordering::Acquire) {
                    let changed = self.changed.notified();
                    if self.settled.load(Ordering::Acquire) {
                        break;
                    }
                    changed.await;
                }
                Ok(())
            })
        }
    }

    struct SyntheticRemoteResource;

    impl ManagedResourceController for SyntheticRemoteResource {
        fn request_stop(&self) {}

        fn force_stop(&self) {}

        fn await_settled(&self) -> ManagedResourceSettlementFuture<'_> {
            Box::pin(std::future::pending())
        }
    }

    fn synthetic_managed_candidate(
        generation: &str,
        service: std::sync::Arc<SyntheticManagedService>,
        resource: std::sync::Arc<SyntheticManagedResource>,
        second_service: bool,
    ) -> crate::managed_service_bus::ManagedGenerationCandidate {
        let controller: std::sync::Arc<dyn ManagedResourceController> = resource;
        let mut candidate = crate::managed_service_bus::ManagedGenerationCandidate::new(
            omegon_traits::RuntimeCompositionGenerationId::new("composition:caller-supplied")
                .unwrap(),
            omegon_traits::RuntimeContributionId::new("feature:caller-supplied").unwrap(),
            omegon_traits::RuntimeContributionGenerationId::new(format!("caller:{generation}"))
                .unwrap(),
            Duration::from_millis(10),
            Duration::from_millis(10),
            vec![
                crate::managed_service_bus::ManagedResourceRegistration::new(
                    omegon_traits::RuntimeContributionResourceId::new(format!(
                        "resource:{generation}"
                    ))
                    .unwrap(),
                    omegon_traits::RuntimeOwnedResourceKind::Task,
                    RuntimeCleanupAssurance::Strict,
                    Vec::new(),
                    controller,
                ),
            ],
        )
        .unwrap();
        candidate
            .add_service(
                omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed").unwrap(),
                omegon_traits::RuntimeServiceInterfaceId::new("interface:synthetic-managed-v1")
                    .unwrap(),
                service,
            )
            .unwrap();
        if second_service {
            candidate
                .add_service(
                    omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed-second")
                        .unwrap(),
                    omegon_traits::RuntimeServiceInterfaceId::new(
                        "interface:synthetic-managed-second-v1",
                    )
                    .unwrap(),
                    std::sync::Arc::new(SyntheticManagedService(2)),
                )
                .unwrap();
        }
        candidate
    }

    #[async_trait]
    impl Feature for InProcessServiceFeature {
        fn name(&self) -> &str {
            "in-process-test"
        }

        fn runtime_contribution_generation_id(
            &self,
        ) -> Option<omegon_traits::RuntimeContributionGenerationId> {
            Some(
                omegon_traits::RuntimeContributionGenerationId::new(self.generation)
                    .expect("test generation is valid"),
            )
        }

        fn runtime_in_process_services(&self) -> Vec<omegon_traits::RuntimeInProcessService> {
            if !self.publish {
                return Vec::new();
            }
            let mut services = vec![
                omegon_traits::RuntimeInProcessService::no_resource_read_service(
                    omegon_traits::RuntimeCapabilityId::new("service:test-read")
                        .expect("test capability is valid"),
                    omegon_traits::RuntimeServiceInterfaceId::new(self.interface)
                        .expect("test interface is valid"),
                    std::sync::Arc::clone(&self.service),
                ),
            ];
            if self.publish_additional {
                services.push(
                    omegon_traits::RuntimeInProcessService::no_resource_read_service(
                        omegon_traits::RuntimeCapabilityId::new("service:test-read-extra").unwrap(),
                        omegon_traits::RuntimeServiceInterfaceId::new("interface:test-read-v1")
                            .unwrap(),
                        std::sync::Arc::clone(&self.service),
                    ),
                );
            }
            services
        }

        fn runtime_lifecycle_policy(&self) -> Option<RuntimeLifecyclePolicy> {
            Some(RuntimeLifecyclePolicy {
                requirement: RuntimeLifecycleRequirement::Optional,
                failure_disposition: RuntimeFailureDisposition::DegradeLocally,
                readiness_timeout_ms: 0,
                heartbeat_timeout_ms: None,
                restart_limit: 0,
            })
        }

        fn runtime_transition_policy(&self) -> Option<RuntimeCompositionTransitionPolicy> {
            Some(RuntimeCompositionTransitionPolicy {
                activation_boundary: RuntimeActivationBoundary::Boot,
                cleanup: RuntimeCleanupRequirement::Strict,
                cleanup_timeout_ms: 0,
            })
        }
    }

    #[async_trait]
    impl Feature for MalformedInProcessServiceFeature {
        fn name(&self) -> &str {
            "in-process-test"
        }

        fn runtime_in_process_services(&self) -> Vec<omegon_traits::RuntimeInProcessService> {
            let mut service = omegon_traits::RuntimeInProcessService::no_resource_read_service(
                omegon_traits::RuntimeCapabilityId::new("service:test-read").unwrap(),
                omegon_traits::RuntimeServiceInterfaceId::new("interface:test-read-v1").unwrap(),
                std::sync::Arc::new(ReadOnlyTestService { value: 3 })
                    as std::sync::Arc<dyn ReadOnlyTestServiceContract>,
            );
            service.capability.kind = omegon_traits::RuntimeCapabilityKind::Tool;
            vec![service]
        }
    }

    #[async_trait]
    impl Feature for AcpInvocationFeature {
        fn name(&self) -> &str {
            "acp-invocation"
        }

        fn runtime_acp_invocations(&self) -> Vec<omegon_traits::RuntimeAcpInvocationDefinition> {
            vec![omegon_traits::RuntimeAcpInvocationDefinition {
                name: "extension_rpc:test".into(),
            }]
        }

        async fn execute_acp_invocation(
            &self,
            name: &str,
            args: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<serde_json::Value> {
            assert_eq!(name, "extension_rpc:test");
            Ok(args)
        }
    }

    #[async_trait]
    impl Feature for ServiceToolFeature {
        fn name(&self) -> &str {
            "service-tool"
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "service_status".into(),
                label: "service status".into(),
                description: "service invocation test".into(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            }]
        }

        fn runtime_tool_surfaces(
            &self,
            tool_name: &str,
        ) -> Option<Vec<omegon_traits::RuntimeSurface>> {
            (tool_name == "service_status").then(|| vec![omegon_traits::RuntimeSurface::Web])
        }

        fn runtime_tool_principals(
            &self,
            tool_name: &str,
        ) -> Option<Vec<omegon_traits::RuntimePrincipalClass>> {
            (tool_name == "service_status")
                .then(|| vec![omegon_traits::RuntimePrincipalClass::Service])
        }

        fn commands(&self) -> Vec<CommandDefinition> {
            vec![CommandDefinition {
                name: "service_command".into(),
                description: "service command surface test".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::READ_ONLY,
                surface: Default::default(),
            }]
        }

        fn runtime_command_surfaces(
            &self,
            command_name: &str,
        ) -> Option<Vec<omegon_traits::RuntimeSurface>> {
            (command_name == "service_command").then(|| vec![omegon_traits::RuntimeSurface::Web])
        }

        fn handle_command(&mut self, name: &str, _args: &str) -> CommandResult {
            if name == "service_command" {
                CommandResult::Handled
            } else {
                CommandResult::NotHandled
            }
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: "service ready".into(),
                }],
                details: json!({}),
            })
        }
    }

    #[async_trait]
    impl Feature for DeclaredPolicyFeature {
        fn name(&self) -> &str {
            self.feature_name
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: self.tool_name.into(),
                label: self.tool_name.into(),
                description: "declared authority test".into(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![],
            }]
        }

        fn runtime_tool_policy(
            &self,
            _tool_name: &str,
        ) -> Option<omegon_traits::RuntimeToolPolicy> {
            let mutates = self.effects.iter().any(|effect| {
                matches!(
                    effect,
                    omegon_traits::RuntimeEffect::FilesystemWrite
                        | omegon_traits::RuntimeEffect::DurableStateWrite
                        | omegon_traits::RuntimeEffect::RuntimeControl
                )
            });
            Some(omegon_traits::RuntimeToolPolicy {
                effects: self.effects.clone(),
                execution: omegon_traits::RuntimeExecutionPolicy {
                    principals: self.principals.clone(),
                    timeout_class: omegon_traits::RuntimeTimeoutClass::Immediate,
                    retry_class: omegon_traits::RuntimeRetryClass::Never,
                    idempotency: omegon_traits::RuntimeIdempotency::NonIdempotent,
                    deduplication: omegon_traits::RuntimeDeduplication::Unsupported,
                    parallelism: omegon_traits::RuntimeParallelism::Serial,
                    transaction: if mutates {
                        omegon_traits::RuntimeTransactionBehavior::IndependentMutation
                    } else {
                        omegon_traits::RuntimeTransactionBehavior::None
                    },
                    mutation_fence: mutates
                        .then(|| Box::new(conservative_mutation_fence(self.tool_name))),
                    max_attempts: None,
                },
            })
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            panic!("admission-only test reached owner")
        }
    }

    struct AlternateNotifierFeature;

    #[async_trait]
    impl Feature for AlternateNotifierFeature {
        fn name(&self) -> &str {
            "alternate-notifier"
        }

        fn commands(&self) -> Vec<CommandDefinition> {
            NotifierFeature.commands()
        }
    }

    struct AliasCommandFeature;

    #[async_trait]
    impl Feature for AliasCommandFeature {
        fn name(&self) -> &str {
            "alias-commands"
        }

        fn commands(&self) -> Vec<CommandDefinition> {
            ["delegate", "subagent"]
                .into_iter()
                .map(|name| CommandDefinition {
                    name: name.into(),
                    description: name.into(),
                    subcommands: vec![],
                    availability: omegon_traits::CommandAvailability::ALL,
                    safety: omegon_traits::CommandSafety::READ_ONLY,
                    surface: Default::default(),
                })
                .collect()
        }

        fn command_aliases(&self) -> Vec<omegon_traits::CommandAlias> {
            vec![omegon_traits::CommandAlias {
                alias: "subagent".into(),
                canonical: "delegate".into(),
            }]
        }
    }

    struct MalformedToolFeature;

    #[async_trait]
    impl Feature for MalformedToolFeature {
        fn name(&self) -> &str {
            "malformed-tool"
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "bad tool".into(),
                label: "bad".into(),
                description: "bad".into(),
                parameters: json!({"type": "object"}),
                capabilities: vec![],
            }]
        }
    }

    struct MissingAliasImplementationFeature;

    #[async_trait]
    impl Feature for MissingAliasImplementationFeature {
        fn name(&self) -> &str {
            "missing-alias"
        }

        fn commands(&self) -> Vec<CommandDefinition> {
            vec![CommandDefinition {
                name: "delegate".into(),
                description: "delegate".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::READ_ONLY,
                surface: Default::default(),
            }]
        }

        fn command_aliases(&self) -> Vec<omegon_traits::CommandAlias> {
            vec![omegon_traits::CommandAlias {
                alias: "missing".into(),
                canonical: "delegate".into(),
            }]
        }
    }

    struct MalformedAliasFeature;

    #[async_trait]
    impl Feature for MalformedAliasFeature {
        fn name(&self) -> &str {
            "malformed-alias"
        }

        fn command_aliases(&self) -> Vec<omegon_traits::CommandAlias> {
            vec![omegon_traits::CommandAlias {
                alias: "alias".into(),
                canonical: "bad canonical".into(),
            }]
        }
    }

    #[test]
    fn finalized_bus_projects_read_only_capability_inventory_without_changing_tools() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        let legacy_tools = bus.tool_definitions();
        let registry = bus.runtime_capability_registry();
        let projected_tools = registry
            .declarations
            .iter()
            .filter(|declaration| declaration.kind == omegon_traits::RuntimeCapabilityKind::Tool)
            .map(|declaration| declaration.invocations[0].name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(projected_tools, vec!["count"]);
        assert_eq!(legacy_tools.len(), projected_tools.len());
        assert!(
            registry.diagnostics.iter().all(|diagnostic| matches!(
                diagnostic,
                omegon_traits::RuntimeCapabilityDiagnostic::DanglingGroupMember { .. }
            )),
            "minimal test bus should expose only expected absent-group diagnostics: {:?}",
            registry.diagnostics
        );
        assert!(
            registry
                .declarations
                .iter()
                .any(|declaration| declaration.id.as_str() == "action:notify")
        );
    }

    #[tokio::test]
    async fn read_only_capability_inventory_does_not_change_legacy_dispatch() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 7 }));
        bus.finalize();

        let before = bus
            .execute_tool(
                "count",
                "before",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("legacy dispatch before inventory projection");
        let registry = bus.runtime_capability_registry();
        let after = bus
            .execute_tool(
                "count",
                "after",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("legacy dispatch after inventory projection");

        assert_eq!(
            serde_json::to_value(&before).expect("serialize pre-projection result"),
            serde_json::to_value(&after).expect("serialize post-projection result")
        );
        assert!(
            registry
                .declarations
                .iter()
                .any(|declaration| declaration.id.as_str() == "tool:count")
        );
    }

    #[test]
    fn negotiated_lifecycle_policy_is_frozen_into_registry() {
        let mut bus = EventBus::new();
        bus.register(Box::new(ExtensionCounterFeature));
        bus.finalize();

        let graph = bus.accepted_graph.as_ref().expect("accepted graph");
        let declaration = graph
            .declarations
            .values()
            .find(|declaration| declaration.id.as_str() == "feature:recro-coe-agent")
            .expect("extension contribution declaration");

        assert_eq!(declaration.lifecycle.readiness_timeout_ms, 2_500);
        assert_eq!(declaration.lifecycle.heartbeat_timeout_ms, Some(10_000));
        assert_eq!(declaration.lifecycle.restart_limit, 3);
        assert_eq!(
            declaration.lifecycle.failure_disposition,
            RuntimeFailureDisposition::Quarantine
        );
        assert_eq!(declaration.transition.cleanup_timeout_ms, 500);
        assert_eq!(
            declaration.transition.cleanup,
            RuntimeCleanupRequirement::Strict
        );

        let projection = bus.composition_diagnostic_projection().unwrap();
        let contribution = projection
            .contributions
            .iter()
            .find(|contribution| contribution.declaration.id.as_str() == "feature:recro-coe-agent")
            .unwrap();
        assert_eq!(
            contribution.health,
            RuntimeContributionLifecycleState::Active
        );
        assert_eq!(
            contribution.cleanup_assurance,
            RuntimeCleanupAssurance::Strict
        );
        assert_eq!(contribution.cleanup_state, RuntimeCleanupState::NotRequired);
        assert!(projection.compatibility_dispatch.parity_verified);
        assert_eq!(projection.compatibility_dispatch.published_bindings, 1);
        assert!(
            projection
                .render_markdown()
                .contains("graph-derived legacy")
        );
    }

    #[test]
    fn plan_survives_lazy_injection_without_prior_use() {
        assert!(
            is_core_tool(crate::tool_registry::core::PLAN),
            "plan must remain callable while durable Workbench instructions require reconciliation"
        );
    }

    #[test]
    fn situational_static_tools_remain_lazy() {
        use crate::tool_registry as reg;

        for name in [
            reg::render::RENDER_DIAGRAM,
            reg::local_inference::ASK_LOCAL_MODEL,
            reg::skills::SKILLS_CREATE,
            reg::secrets::SECRET_SET,
        ] {
            assert!(
                !is_core_tool(name),
                "situational tool '{name}' should not bypass lazy injection"
            );
        }
    }

    #[test]
    fn unknown_invocation_receives_no_execution_lease() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        let admission = crate::invocation_service::InvocationService::admit_tool(
            &bus,
            "missing",
            crate::invocation_service::InvocationAdmissionRequest {
                call_id: "unknown-call",
                visible_tool_name: "missing",
                args: &json!({}),
                scope: crate::invocation_service::InvocationScope::default(),
                permission_policy: None,
                permission_role: None,
            },
        );
        let crate::invocation_service::InvocationAdmission::Denied(denial) = admission else {
            panic!("unknown invocation must not receive a lease")
        };
        assert_eq!(
            denial.code,
            crate::invocation_service::InvocationDenialCode::UnknownInvocation
        );
    }

    #[test]
    fn operator_command_admission_uses_declared_kind_principal_and_surface() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.finalize();
        let request = |scope| crate::invocation_service::InvocationRequest {
            call_id: "notify-call",
            scope,
            permission_policy: None,
            permission_role: None,
            permission_name: "notify",
            permission_subjects: &[],
        };

        let model_admission = crate::invocation_service::InvocationService::admit_invocation(
            &bus,
            omegon_traits::RuntimeInvocationKind::Command,
            "notify",
            request(crate::invocation_service::InvocationScope::default()),
        );
        assert!(matches!(
            model_admission,
            crate::invocation_service::InvocationAdmission::Denied(
                crate::invocation_service::InvocationDenial {
                    code: crate::invocation_service::InvocationDenialCode::UnsupportedSurface,
                    ..
                }
            )
        ));

        let scope = crate::invocation_service::InvocationScope {
            principal: "operator".into(),
            principal_class: omegon_traits::RuntimePrincipalClass::Operator,
            surface: omegon_traits::RuntimeSurface::Tui,
            ..Default::default()
        };
        let crate::invocation_service::InvocationAdmission::Lease(lease) =
            crate::invocation_service::InvocationService::admit_invocation(
                &bus,
                omegon_traits::RuntimeInvocationKind::Command,
                "notify",
                request(scope),
            )
        else {
            panic!("declared operator command should receive a lease")
        };
        let result = bus
            .dispatch_command_with_lease(&lease, "notify", "notify-call", "hello")
            .unwrap();
        assert!(matches!(
            result,
            CommandResult::Display(message) if message == "Notified: hello"
        ));
        assert_eq!(
            lease.terminal(),
            crate::invocation_service::LeaseTerminal::Completed
        );
    }

    #[tokio::test]
    async fn internal_dispatch_requires_and_settles_an_internal_lease() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 3 }));
        bus.register_internal_tool("internal_count", "counter");
        bus.finalize();
        let scope = crate::invocation_service::InvocationScope {
            principal: "kernel:test".into(),
            principal_class: omegon_traits::RuntimePrincipalClass::Internal,
            surface: omegon_traits::RuntimeSurface::Internal,
            ..Default::default()
        };
        let crate::invocation_service::InvocationAdmission::Lease(lease) =
            crate::invocation_service::InvocationService::admit_invocation(
                &bus,
                omegon_traits::RuntimeInvocationKind::Internal,
                "internal_count",
                crate::invocation_service::InvocationRequest {
                    call_id: "internal-call",
                    scope: scope.clone(),
                    permission_policy: None,
                    permission_role: None,
                    permission_name: "internal_count",
                    permission_subjects: &[],
                },
            )
        else {
            panic!("declared internal invocation should receive a lease")
        };

        let result = bus
            .execute_internal_with_lease(
                &lease,
                "internal_count",
                "internal-call",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.content[0].as_text(), Some("count: 3"));
        assert_eq!(
            lease.terminal(),
            crate::invocation_service::LeaseTerminal::Completed
        );

        let result = bus
            .invoke_internal(
                "internal_count",
                "internal-call-2",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
                scope,
            )
            .await
            .unwrap();
        assert_eq!(result.content[0].as_text(), Some("count: 3"));
    }

    #[tokio::test]
    async fn service_tool_uses_declared_principal_surface_and_lease() {
        let mut bus = EventBus::new();
        bus.register(Box::new(ServiceToolFeature));
        bus.finalize();
        let scope = crate::invocation_service::InvocationScope {
            principal: "managed-worker".into(),
            principal_class: omegon_traits::RuntimePrincipalClass::Service,
            surface: omegon_traits::RuntimeSurface::Web,
            ..Default::default()
        };

        let result = bus
            .invoke_tool(
                "service_status",
                "service-call",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
                scope,
            )
            .await
            .unwrap();
        assert_eq!(result.content[0].as_text(), Some("service ready"));

        let command_scope = crate::invocation_service::InvocationScope {
            principal: "web-operator".into(),
            principal_class: omegon_traits::RuntimePrincipalClass::Operator,
            surface: omegon_traits::RuntimeSurface::Web,
            ..Default::default()
        };
        assert!(matches!(
            bus.invoke_command(
                "service_command",
                "service-command-call",
                "",
                command_scope,
                None,
            )
            .unwrap(),
            CommandResult::Handled
        ));

        let denial = crate::invocation_service::InvocationService::admit_tool(
            &bus,
            "service_status",
            crate::invocation_service::InvocationAdmissionRequest {
                call_id: "model-call",
                visible_tool_name: "service_status",
                args: &json!({}),
                scope: crate::invocation_service::InvocationScope::default(),
                permission_policy: None,
                permission_role: None,
            },
        );
        assert!(matches!(
            denial,
            crate::invocation_service::InvocationAdmission::Denied(
                crate::invocation_service::InvocationDenial {
                    code: crate::invocation_service::InvocationDenialCode::UnsupportedSurface,
                    ..
                }
            )
        ));
    }

    #[tokio::test]
    async fn acp_transport_invocation_uses_declared_operator_lease() {
        let mut bus = EventBus::new();
        bus.register(Box::new(AcpInvocationFeature));
        bus.try_finalize().unwrap();
        let scope = crate::invocation_service::InvocationScope {
            principal: "acp-client".into(),
            principal_class: omegon_traits::RuntimePrincipalClass::Operator,
            surface: omegon_traits::RuntimeSurface::Acp,
            ..Default::default()
        };

        let result = bus
            .invoke_acp(
                "extension_rpc:test",
                "acp-call",
                json!({"method": "ping", "params": {"value": 7}}),
                tokio_util::sync::CancellationToken::new(),
                scope,
            )
            .await
            .unwrap();

        assert_eq!(result["method"], "ping");
        assert_eq!(result["params"]["value"], 7);
    }

    #[test]
    fn rbac_denial_cannot_be_widened_by_policy_allow() {
        let mut bus = EventBus::new();
        bus.register(Box::new(PanicToolFeature("write")));
        bus.finalize();
        let mut policy = crate::permissions::LayeredPermissionPolicy::default();
        policy.project.tools.insert(
            "write".into(),
            crate::permissions::ToolPermissionRule::Action(
                crate::permissions::PermissionAction::Allow,
            ),
        );

        let admission = crate::invocation_service::InvocationService::admit_tool(
            &bus,
            "write",
            crate::invocation_service::InvocationAdmissionRequest {
                call_id: "write-call",
                visible_tool_name: "write",
                args: &json!({}),
                scope: crate::invocation_service::InvocationScope::default(),
                permission_policy: Some(&policy),
                permission_role: styrene_rbac::Role::from_name("monitor"),
            },
        );
        let crate::invocation_service::InvocationAdmission::Denied(denial) = admission else {
            panic!("RBAC denial must not receive a lease")
        };
        assert_eq!(
            denial.code,
            crate::invocation_service::InvocationDenialCode::RbacDenied
        );
    }

    #[test]
    fn rbac_authority_comes_from_declared_effects_not_tool_name() {
        let admission = |feature: DeclaredPolicyFeature| {
            let tool_name = feature.tool_name;
            let mut bus = EventBus::new();
            bus.register(Box::new(feature));
            bus.finalize();
            crate::invocation_service::InvocationService::admit_tool(
                &bus,
                tool_name,
                crate::invocation_service::InvocationAdmissionRequest {
                    call_id: "rbac-call",
                    visible_tool_name: tool_name,
                    args: &json!({}),
                    scope: crate::invocation_service::InvocationScope::default(),
                    permission_policy: None,
                    permission_role: styrene_rbac::Role::from_name("monitor"),
                },
            )
        };

        assert!(matches!(
            admission(DeclaredPolicyFeature {
                feature_name: "benign-name-writer",
                tool_name: "lookup",
                effects: vec![omegon_traits::RuntimeEffect::FilesystemWrite],
                principals: vec![omegon_traits::RuntimePrincipalClass::Model],
            }),
            crate::invocation_service::InvocationAdmission::Denied(
                crate::invocation_service::InvocationDenial {
                    code: crate::invocation_service::InvocationDenialCode::RbacDenied,
                    ..
                }
            )
        ));
        assert!(matches!(
            admission(DeclaredPolicyFeature {
                feature_name: "dangerous-name-reader",
                tool_name: "bash",
                effects: vec![omegon_traits::RuntimeEffect::FilesystemRead],
                principals: vec![omegon_traits::RuntimePrincipalClass::Model],
            }),
            crate::invocation_service::InvocationAdmission::Lease(_)
        ));
    }

    #[test]
    fn principal_display_label_cannot_widen_declared_principal_class() {
        let mut bus = EventBus::new();
        bus.register(Box::new(DeclaredPolicyFeature {
            feature_name: "operator-only",
            tool_name: "operator_task",
            effects: vec![],
            principals: vec![omegon_traits::RuntimePrincipalClass::Operator],
        }));
        bus.finalize();
        let scope = crate::invocation_service::InvocationScope {
            principal: "admin".into(),
            ..Default::default()
        };
        let admission = crate::invocation_service::InvocationService::admit_tool(
            &bus,
            "operator_task",
            crate::invocation_service::InvocationAdmissionRequest {
                call_id: "principal-call",
                visible_tool_name: "operator_task",
                args: &json!({}),
                scope,
                permission_policy: None,
                permission_role: None,
            },
        );
        assert!(matches!(
            admission,
            crate::invocation_service::InvocationAdmission::Denied(
                crate::invocation_service::InvocationDenial {
                    code: crate::invocation_service::InvocationDenialCode::RbacDenied,
                    ..
                }
            )
        ));
    }

    #[tokio::test]
    async fn stale_generation_lease_is_rejected_before_owner_execution() {
        let mut bus = EventBus::new();
        bus.register(Box::new(PanicToolFeature("stale_test")));
        bus.finalize();
        let lease = admitted_test_lease(&bus, "stale-call", "stale_test");
        lease.claim_dispatch("stale-call", "stale_test").unwrap();

        bus.register(Box::new(DisplayNameFeature("additional")));
        bus.try_finalize().unwrap();
        let error = bus
            .execute_tool_with_lease(
                &lease,
                "stale_test",
                "stale-call",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
                omegon_traits::ToolProgressSink::noop(),
                omegon_traits::ToolExecutionContext::default(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("invocation:stale_generation"));
        assert_eq!(
            lease.terminal(),
            crate::invocation_service::LeaseTerminal::Revoked
        );
    }

    #[tokio::test]
    async fn rejected_candidate_preserves_existing_execution_lease() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 7 }));
        bus.finalize();
        let lease = admitted_test_lease(&bus, "count-call", "count");
        bus.register(Box::new(ExtensionCounterFeature));
        assert!(bus.try_finalize().is_err());

        lease.claim_dispatch("count-call", "count").unwrap();
        let result = bus
            .execute_tool_with_lease(
                &lease,
                "count",
                "count-call",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
                omegon_traits::ToolProgressSink::noop(),
                omegon_traits::ToolExecutionContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(result.content[0].as_text(), Some("count: 7"));
        assert!(lease.close(crate::invocation_service::LeaseTerminal::Completed));
    }

    fn admitted_test_lease(
        bus: &EventBus,
        call_id: &str,
        tool_name: &str,
    ) -> crate::invocation_service::ExecutionLease {
        match crate::invocation_service::InvocationService::admit_tool(
            bus,
            tool_name,
            crate::invocation_service::InvocationAdmissionRequest {
                call_id,
                visible_tool_name: tool_name,
                args: &json!({}),
                scope: crate::invocation_service::InvocationScope::default(),
                permission_policy: None,
                permission_role: None,
            },
        ) {
            crate::invocation_service::InvocationAdmission::Lease(lease) => lease,
            crate::invocation_service::InvocationAdmission::ApprovalRequired(_) => {
                panic!("test invocation unexpectedly requires approval")
            }
            crate::invocation_service::InvocationAdmission::Denied(denial) => {
                panic!("test invocation denied: {}", denial.message)
            }
        }
    }

    #[test]
    fn duplicate_tool_owner_is_rejected_independent_of_registration_order() {
        let build = |extension_first| {
            let mut bus = EventBus::new();
            if extension_first {
                bus.register(Box::new(ExtensionCounterFeature));
                bus.register(Box::new(CounterFeature { event_count: 0 }));
            } else {
                bus.register(Box::new(CounterFeature { event_count: 0 }));
                bus.register(Box::new(ExtensionCounterFeature));
            }
            bus.try_finalize().unwrap_err().to_string()
        };

        let first = build(true);
        let second = build(false);
        assert_eq!(first, second);
        assert!(first.contains("graph:duplicate_owner"));
        assert!(first.contains("graph:ambiguous_binding"));
    }

    #[tokio::test]
    async fn failed_candidate_keeps_previous_graph_features_and_dispatch() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 7 }));
        bus.finalize();
        let accepted_generation = bus.composition_generation_id().unwrap().clone();
        bus.register(Box::new(ExtensionCounterFeature));

        assert_eq!(bus.feature_names(), vec!["counter"]);
        assert!(bus.try_finalize().is_err());
        assert_eq!(bus.composition_generation_id(), Some(&accepted_generation));
        assert!(
            bus.composition_diagnostic_projection()
                .unwrap()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == "graph:duplicate_owner")
        );
        assert_eq!(bus.feature_names(), vec!["counter"]);
        let result = bus
            .execute_tool(
                "count",
                "after-rejection",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.content[0].as_text(), Some("count: 7"));

        bus.register(Box::new(DisplayNameFeature("additional")));
        bus.try_finalize().unwrap();
        assert_ne!(bus.composition_generation_id(), Some(&accepted_generation));
        assert!(
            bus.composition_diagnostic_projection()
                .unwrap()
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn managed_diagnostic_no_resource_contribution_remains_not_required() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        let projection = bus.composition_diagnostic_projection().unwrap();
        assert!(projection.managed_owners.is_empty());
        assert!(projection.contributions.iter().all(|contribution| {
            contribution.health == RuntimeContributionLifecycleState::Active
                && contribution.cleanup_state == RuntimeCleanupState::NotRequired
        }));
        let json = serde_json::to_value(&projection).unwrap();
        assert!(json.get("managed_owners").is_none());
    }

    #[tokio::test]
    async fn managed_diagnostic_graph_projection_uses_owner_lifecycle_without_pointer_leaks() {
        let mut bus = EventBus::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-diagnostic-v1",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "caller-diagnostic-v1",
                std::sync::Arc::new(SyntheticManagedService(1)),
                SyntheticManagedResource::new(),
                false,
            ),
        )
        .unwrap();
        bus.try_finalize_managed().await.unwrap();

        let projection = bus.composition_diagnostic_projection().unwrap();
        assert_eq!(projection.managed_owners.len(), 1);
        let owner = &projection.managed_owners[0];
        assert_eq!(
            owner.disposition,
            crate::surfaces::diagnostics::ManagedOwnerDisposition::Published
        );
        assert_eq!(
            owner.lifecycle.state,
            RuntimeContributionLifecycleState::Active
        );
        assert_eq!(owner.lifecycle.cleanup_state, RuntimeCleanupState::Pending);
        let contribution = projection
            .contributions
            .iter()
            .find(|contribution| contribution.declaration.id == owner.lifecycle.contribution_id)
            .unwrap();
        assert_eq!(contribution.cleanup_state, RuntimeCleanupState::Pending);
        let output = format!(
            "{}\n{}",
            serde_json::to_string(&projection).unwrap(),
            projection.render_markdown()
        );
        assert!(output.contains("disposition=published"));
        assert!(output.contains("kind=task"));
        assert!(!output.contains("controller_identity"));
        assert!(!output.contains("implementation_identity"));
        assert!(!output.contains("0x"));

        let shutdown = bus.shutdown_managed_services().await;
        assert!(shutdown.generations[0].result.is_ok());
        let projection = bus.composition_diagnostic_projection().unwrap();
        let owner = projection.managed_owners.last().unwrap();
        assert_eq!(
            owner.lifecycle.state,
            RuntimeContributionLifecycleState::Retired
        );
        assert_eq!(owner.lifecycle.cleanup_state, RuntimeCleanupState::Settled);
        let contribution = projection
            .contributions
            .iter()
            .find(|contribution| contribution.declaration.id == owner.lifecycle.contribution_id)
            .unwrap();
        assert_eq!(
            contribution.health,
            RuntimeContributionLifecycleState::Retired
        );
        assert_eq!(contribution.cleanup_state, RuntimeCleanupState::Settled);
    }

    #[tokio::test]
    async fn cln_002_remote_cleanup_projection_preserves_evidence_boundary() {
        let mut bus = EventBus::new();
        bus.register(Box::new(SyntheticBestEffortManagedFeature));
        let host_resources = [
            ("task", omegon_traits::RuntimeOwnedResourceKind::Task),
            ("socket", omegon_traits::RuntimeOwnedResourceKind::Socket),
            (
                "subscription",
                omegon_traits::RuntimeOwnedResourceKind::Subscription,
            ),
        ]
        .into_iter()
        .map(|(name, kind)| {
            let controller: std::sync::Arc<dyn ManagedResourceController> =
                SyntheticManagedResource::new();
            crate::managed_service_bus::ManagedResourceRegistration::new(
                omegon_traits::RuntimeContributionResourceId::new(format!("resource:{name}"))
                    .unwrap(),
                kind,
                RuntimeCleanupAssurance::BestEffort,
                Vec::new(),
                controller,
            )
        });
        let remote: std::sync::Arc<dyn ManagedResourceController> =
            std::sync::Arc::new(SyntheticRemoteResource);
        let resources = host_resources
            .chain(std::iter::once(
                crate::managed_service_bus::ManagedResourceRegistration::new(
                    omegon_traits::RuntimeContributionResourceId::new("resource:remote-peer")
                        .unwrap(),
                    omegon_traits::RuntimeOwnedResourceKind::RemoteService,
                    RuntimeCleanupAssurance::BestEffort,
                    Vec::new(),
                    remote,
                ),
            ))
            .collect();
        let mut candidate = crate::managed_service_bus::ManagedGenerationCandidate::new(
            omegon_traits::RuntimeCompositionGenerationId::new("composition:caller-supplied")
                .unwrap(),
            omegon_traits::RuntimeContributionId::new("feature:caller-supplied").unwrap(),
            omegon_traits::RuntimeContributionGenerationId::new("caller:remote-cleanup-v1")
                .unwrap(),
            Duration::from_millis(10),
            Duration::from_millis(10),
            resources,
        )
        .unwrap();
        candidate
            .add_service(
                omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed").unwrap(),
                omegon_traits::RuntimeServiceInterfaceId::new("interface:synthetic-managed-v1")
                    .unwrap(),
                std::sync::Arc::new(SyntheticManagedService(1)),
            )
            .unwrap();
        bus.stage_managed_generation("synthetic-managed", candidate)
            .unwrap();
        bus.try_finalize_managed().await.unwrap();

        let _ = bus.shutdown_managed_services().await;
        let projection = bus.composition_diagnostic_projection().unwrap();
        let owner = projection.managed_owners.last().unwrap();
        assert_eq!(
            owner.lifecycle.cleanup_assurance,
            RuntimeCleanupAssurance::BestEffort
        );
        assert_eq!(
            owner.lifecycle.cleanup_state,
            RuntimeCleanupState::Unverified
        );
        assert!(
            owner
                .resources
                .iter()
                .filter(|resource| {
                    resource.record.kind != omegon_traits::RuntimeOwnedResourceKind::RemoteService
                })
                .all(|resource| resource.record.cleanup_state == RuntimeCleanupState::Settled)
        );
        let remote = owner
            .resources
            .iter()
            .find(|resource| {
                resource.record.kind == omegon_traits::RuntimeOwnedResourceKind::RemoteService
            })
            .unwrap();
        assert_eq!(
            remote.record.cleanup_assurance,
            RuntimeCleanupAssurance::BestEffort
        );
        assert_eq!(remote.record.cleanup_state, RuntimeCleanupState::Unverified);
        remote.record.validate().unwrap();

        let acp_visible = format!(
            "{}\n{}",
            serde_json::to_string(&projection).unwrap(),
            projection.render_markdown()
        );
        assert!(acp_visible.contains("kind=remote_service cleanup=best_effort/unverified"));
        assert!(!acp_visible.contains("kind=remote_service cleanup=strict/settled"));
    }

    #[tokio::test]
    async fn managed_graph_initial_publication_supports_typed_lookup() {
        let mut bus = EventBus::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v1",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        let resource = SyntheticManagedResource::new();
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "caller-generation-v1",
                std::sync::Arc::new(SyntheticManagedService(1)),
                std::sync::Arc::clone(&resource),
                false,
            ),
        )
        .unwrap();

        bus.try_finalize_managed().await.unwrap();

        let capability =
            omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed").unwrap();
        let interface =
            omegon_traits::RuntimeServiceInterfaceId::new("interface:synthetic-managed-v1")
                .unwrap();
        let handle = bus
            .managed_service::<SyntheticManagedService>(&capability, &interface)
            .unwrap()
            .unwrap();
        assert_eq!(handle.invoke(()).await.unwrap(), 1);
        assert_eq!(handle.owner.as_str(), "feature:synthetic-managed");
        assert_eq!(
            handle.generation_id.as_str(),
            "contribution:synthetic-managed-v1"
        );
        assert_eq!(
            bus.accepted_graph
                .as_ref()
                .unwrap()
                .capability_owners
                .get(&capability)
                .unwrap()
                .as_str(),
            "feature:synthetic-managed"
        );
        assert!(
            bus.runtime_capability_registry()
                .declarations
                .iter()
                .any(|declaration| declaration.id == capability
                    && declaration.kind == omegon_traits::RuntimeCapabilityKind::InProcessService)
        );
        assert_eq!(resource.stops.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn dynamic_publication_retains_unrelated_managed_generation() {
        let mut bus = EventBus::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v1",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        let resource = SyntheticManagedResource::new();
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "caller-generation-v1",
                std::sync::Arc::new(SyntheticManagedService(1)),
                std::sync::Arc::clone(&resource),
                false,
            ),
        )
        .unwrap();
        bus.try_finalize_managed().await.unwrap();

        bus.register(Box::new(DisplayNameFeature("additional")));
        let publication = bus.prepare_dynamic_publication().unwrap();
        assert_eq!(bus.feature_names(), vec!["synthetic-managed"]);
        bus.commit_dynamic_publication(publication);

        let capability =
            omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed").unwrap();
        let interface =
            omegon_traits::RuntimeServiceInterfaceId::new("interface:synthetic-managed-v1")
                .unwrap();
        let handle = bus
            .managed_service::<SyntheticManagedService>(&capability, &interface)
            .unwrap()
            .unwrap();
        assert_eq!(handle.invoke(()).await.unwrap(), 1);
        assert_eq!(bus.feature_names(), vec!["synthetic-managed", "additional"]);
        assert_eq!(resource.stops.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn managed_graph_two_services_share_one_contribution_generation() {
        let mut bus = EventBus::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v1",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "ignored-generation",
                std::sync::Arc::new(SyntheticManagedService(1)),
                SyntheticManagedResource::new(),
                true,
            ),
        )
        .unwrap();
        bus.try_finalize_managed().await.unwrap();

        let metadata = bus.managed_service_metadata();
        assert_eq!(metadata.len(), 2);
        assert!(metadata.iter().all(|service| {
            service.owner.as_str() == "feature:synthetic-managed"
                && service.generation_id.as_str() == "contribution:synthetic-managed-v1"
        }));
        let second = bus
            .managed_service::<SyntheticManagedService>(
                &omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed-second")
                    .unwrap(),
                &omegon_traits::RuntimeServiceInterfaceId::new(
                    "interface:synthetic-managed-second-v1",
                )
                .unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(second.invoke(()).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn managed_graph_policy_rejection_preserves_graph_and_cleans_candidate() {
        let mut bus = EventBus::new();
        let old_service = std::sync::Arc::new(SyntheticManagedService(1));
        let old_resource = SyntheticManagedResource::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v1",
            boundary: RuntimeActivationBoundary::QuiescentSession,
            cleanup_timeout_ms: 10,
        }));
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "ignored-v1",
                std::sync::Arc::clone(&old_service),
                std::sync::Arc::clone(&old_resource),
                false,
            ),
        )
        .unwrap();
        bus.try_finalize_managed().await.unwrap();
        let accepted_generation = bus.composition_generation_id().unwrap().clone();

        let rejected_resource = SyntheticManagedResource::new();
        bus.replace_feature(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v2",
            boundary: RuntimeActivationBoundary::QuiescentSession,
            cleanup_timeout_ms: 10,
        }));
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "ignored-v2",
                std::sync::Arc::new(SyntheticManagedService(2)),
                std::sync::Arc::clone(&rejected_resource),
                false,
            ),
        )
        .unwrap();

        let error = bus.try_finalize_managed().await.unwrap_err().to_string();
        assert!(error.contains("quiescence proof issuer"), "{error}");
        assert!(error.contains("rollback cleanup reports"), "{error}");
        assert_eq!(bus.composition_generation_id(), Some(&accepted_generation));
        assert_eq!(old_resource.stops.load(Ordering::Acquire), 0);
        assert_eq!(rejected_resource.stops.load(Ordering::Acquire), 1);
        let retained = bus
            .managed_service::<SyntheticManagedService>(
                &omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed").unwrap(),
                &omegon_traits::RuntimeServiceInterfaceId::new("interface:synthetic-managed-v1")
                    .unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(retained.invoke(()).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn managed_graph_sync_finalize_fails_closed_without_touching_resources() {
        let mut bus = EventBus::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v1",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        let resource = SyntheticManagedResource::new();
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "ignored-v1",
                std::sync::Arc::new(SyntheticManagedService(1)),
                std::sync::Arc::clone(&resource),
                false,
            ),
        )
        .unwrap();

        let error = bus.try_finalize().unwrap_err().to_string();
        assert!(error.contains("try_finalize_managed"), "{error}");
        assert!(bus.accepted_graph.is_none());
        assert_eq!(resource.stops.load(Ordering::Acquire), 0);

        bus.try_finalize_managed().await.unwrap();
        assert_eq!(resource.stops.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn managed_graph_active_generation_cannot_be_implicitly_removed() {
        let mut bus = EventBus::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v1",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        let resource = SyntheticManagedResource::new();
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "ignored-v1",
                std::sync::Arc::new(SyntheticManagedService(1)),
                std::sync::Arc::clone(&resource),
                false,
            ),
        )
        .unwrap();
        bus.try_finalize_managed().await.unwrap();

        let sync_error = bus.try_finalize().unwrap_err().to_string();
        assert!(sync_error.contains("try_finalize_managed"), "{sync_error}");
        let async_error = bus.try_finalize_managed().await.unwrap_err().to_string();
        assert!(
            async_error.contains("removing active managed generation"),
            "{async_error}"
        );
        assert_eq!(resource.stops.load(Ordering::Acquire), 0);
        let retained = bus
            .managed_service::<SyntheticManagedService>(
                &omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed").unwrap(),
                &omegon_traits::RuntimeServiceInterfaceId::new("interface:synthetic-managed-v1")
                    .unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(retained.invoke(()).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn managed_graph_cleanup_assurance_must_match_resource_controllers() {
        let mut bus = EventBus::new();
        bus.register(Box::new(SyntheticBestEffortManagedFeature));
        let resource = SyntheticManagedResource::new();
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "ignored-v1",
                std::sync::Arc::new(SyntheticManagedService(1)),
                std::sync::Arc::clone(&resource),
                false,
            ),
        )
        .unwrap();

        let error = bus.try_finalize_managed().await.unwrap_err().to_string();
        assert!(error.contains("cleanup assurance"), "{error}");
        assert!(bus.accepted_graph.is_none());
        assert_eq!(resource.stops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn managed_graph_and_direct_sidecar_cannot_claim_the_same_capability() {
        let capability = omegon_traits::RuntimeCapabilityId::new("service:test-read").unwrap();
        let managed_interface =
            omegon_traits::RuntimeServiceInterfaceId::new("interface:synthetic-managed-v1")
                .unwrap();
        let direct_candidate = |resource: std::sync::Arc<SyntheticManagedResource>| {
            let controller: std::sync::Arc<dyn ManagedResourceController> = resource;
            crate::managed_service_bus::ManagedServiceCandidate::new(
                omegon_traits::RuntimeCompositionGenerationId::new("composition:direct").unwrap(),
                capability.clone(),
                managed_interface.clone(),
                omegon_traits::RuntimeContributionId::new("feature:direct").unwrap(),
                omegon_traits::RuntimeContributionGenerationId::new("contribution:direct-v1")
                    .unwrap(),
                Duration::from_millis(10),
                Duration::from_millis(10),
                vec![
                    crate::managed_service_bus::ManagedResourceRegistration::new(
                        omegon_traits::RuntimeContributionResourceId::new("resource:direct")
                            .unwrap(),
                        omegon_traits::RuntimeOwnedResourceKind::Task,
                        RuntimeCleanupAssurance::Strict,
                        Vec::new(),
                        controller,
                    ),
                ],
                std::sync::Arc::new(SyntheticManagedService(1)),
            )
            .unwrap()
        };

        let mut graph_first = EventBus::new();
        graph_first.register(Box::new(InProcessServiceFeature {
            generation: "service:test-v1",
            interface: "interface:test-read-v1",
            service: std::sync::Arc::new(ReadOnlyTestService { value: 7 })
                as std::sync::Arc<dyn ReadOnlyTestServiceContract>,
            publish: true,
            publish_additional: false,
        }));
        graph_first.try_finalize().unwrap();
        let rejected_resource = SyntheticManagedResource::new();
        assert!(matches!(
            graph_first
                .publish_managed_service(direct_candidate(std::sync::Arc::clone(
                    &rejected_resource
                )))
                .await,
            crate::managed_service_bus::ManagedServicePublicationOutcome::Rejected { .. }
        ));
        assert_eq!(rejected_resource.stops.load(Ordering::Acquire), 1);
        assert_eq!(
            graph_first
                .in_process_service::<dyn ReadOnlyTestServiceContract>(
                    &capability,
                    &omegon_traits::RuntimeServiceInterfaceId::new("interface:test-read-v1")
                        .unwrap(),
                )
                .unwrap()
                .unwrap()
                .service
                .value(),
            7
        );

        let mut direct_first = EventBus::new();
        let retained_resource = SyntheticManagedResource::new();
        assert!(matches!(
            direct_first
                .publish_managed_service(direct_candidate(std::sync::Arc::clone(
                    &retained_resource
                )))
                .await,
            crate::managed_service_bus::ManagedServicePublicationOutcome::Published { .. }
        ));
        direct_first.register(Box::new(InProcessServiceFeature {
            generation: "service:test-v1",
            interface: "interface:test-read-v1",
            service: std::sync::Arc::new(ReadOnlyTestService { value: 9 })
                as std::sync::Arc<dyn ReadOnlyTestServiceContract>,
            publish: true,
            publish_additional: false,
        }));
        let error = direct_first.try_finalize().unwrap_err().to_string();
        assert!(
            error.contains("collides with direct managed service"),
            "{error}"
        );
        assert_eq!(retained_resource.stops.load(Ordering::Acquire), 0);
        assert_eq!(
            direct_first
                .managed_service::<SyntheticManagedService>(&capability, &managed_interface)
                .unwrap()
                .unwrap()
                .invoke(())
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn managed_graph_boot_change_rejects_before_old_admission_closes() {
        let mut bus = EventBus::new();
        let old_resource = SyntheticManagedResource::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v1",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "ignored-v1",
                std::sync::Arc::new(SyntheticManagedService(1)),
                std::sync::Arc::clone(&old_resource),
                false,
            ),
        )
        .unwrap();
        bus.try_finalize_managed().await.unwrap();

        let rejected_resource = SyntheticManagedResource::new();
        bus.replace_feature(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v2",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "ignored-v2",
                std::sync::Arc::new(SyntheticManagedService(2)),
                std::sync::Arc::clone(&rejected_resource),
                false,
            ),
        )
        .unwrap();

        let error = bus.try_finalize_managed().await.unwrap_err().to_string();
        assert!(error.contains("Boot managed generation"), "{error}");
        let retained = bus
            .managed_service::<SyntheticManagedService>(
                &omegon_traits::RuntimeCapabilityId::new("service:synthetic-managed").unwrap(),
                &omegon_traits::RuntimeServiceInterfaceId::new("interface:synthetic-managed-v1")
                    .unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(retained.invoke(()).await.unwrap(), 1);
        assert_eq!(old_resource.stops.load(Ordering::Acquire), 0);
        assert_eq!(rejected_resource.stops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn managed_graph_exact_transfer_has_no_cleanup() {
        let mut bus = EventBus::new();
        let service = std::sync::Arc::new(SyntheticManagedService(1));
        let resource = SyntheticManagedResource::new();
        bus.register(Box::new(SyntheticManagedFeature {
            generation: "contribution:synthetic-managed-v1",
            boundary: RuntimeActivationBoundary::Boot,
            cleanup_timeout_ms: 10,
        }));
        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "same-resource-generation",
                std::sync::Arc::clone(&service),
                std::sync::Arc::clone(&resource),
                false,
            ),
        )
        .unwrap();
        bus.try_finalize_managed().await.unwrap();
        let first_generation = bus.composition_generation_id().unwrap().clone();

        bus.stage_managed_generation(
            "synthetic-managed",
            synthetic_managed_candidate(
                "same-resource-generation",
                service,
                std::sync::Arc::clone(&resource),
                false,
            ),
        )
        .unwrap();
        bus.try_finalize_managed().await.unwrap();

        assert_ne!(bus.composition_generation_id(), Some(&first_generation));
        assert_eq!(resource.stops.load(Ordering::Acquire), 0);
    }

    #[test]
    fn in_process_service_publication_is_atomic_and_generation_bound() {
        let capability = omegon_traits::RuntimeCapabilityId::new("service:test-read").unwrap();
        let interface =
            omegon_traits::RuntimeServiceInterfaceId::new("interface:test-read-v1").unwrap();
        let mut bus = EventBus::new();
        bus.register(Box::new(InProcessServiceFeature {
            generation: "service:test-v1",
            interface: "interface:test-read-v1",
            service: std::sync::Arc::new(ReadOnlyTestService { value: 1 })
                as std::sync::Arc<dyn ReadOnlyTestServiceContract>,
            publish: true,
            publish_additional: false,
        }));
        bus.try_finalize().unwrap();

        let first = bus
            .in_process_service::<dyn ReadOnlyTestServiceContract>(&capability, &interface)
            .unwrap()
            .unwrap();
        assert_eq!(first.capability_id, capability);
        assert_eq!(first.owner.as_str(), "feature:in-process-test");
        assert_eq!(first.generation_id.as_str(), "service:test-v1");
        assert_eq!(first.service.value(), 1);
        assert!(
            bus.runtime_capability_registry()
                .declarations
                .iter()
                .any(|declaration| declaration.id == capability
                    && declaration.kind == omegon_traits::RuntimeCapabilityKind::InProcessService)
        );
        let wrong_interface =
            omegon_traits::RuntimeServiceInterfaceId::new("interface:wrong-v1").unwrap();
        assert!(
            bus.in_process_service::<dyn ReadOnlyTestServiceContract>(
                &capability,
                &wrong_interface
            )
            .is_err()
        );

        let second_service = std::sync::Arc::new(ReadOnlyTestService { value: 2 })
            as std::sync::Arc<dyn ReadOnlyTestServiceContract>;
        bus.replace_feature(Box::new(InProcessServiceFeature {
            generation: "service:test-v2",
            interface: "interface:test-read-v1",
            service: std::sync::Arc::clone(&second_service),
            publish: true,
            publish_additional: false,
        }));
        bus.try_finalize().unwrap();
        let second = bus
            .in_process_service::<dyn ReadOnlyTestServiceContract>(&capability, &interface)
            .unwrap()
            .unwrap();
        assert_eq!(second.generation_id.as_str(), "service:test-v2");
        assert_eq!(second.service.value(), 2);
        assert_eq!(first.generation_id.as_str(), "service:test-v1");
        assert_eq!(first.service.value(), 1);

        bus.replace_feature(Box::new(MalformedInProcessServiceFeature));
        assert!(bus.try_finalize().is_err());
        let retained = bus
            .in_process_service::<dyn ReadOnlyTestServiceContract>(&capability, &interface)
            .unwrap()
            .unwrap();
        assert_eq!(retained.generation_id.as_str(), "service:test-v2");
        assert_eq!(retained.service.value(), 2);

        bus.replace_feature(Box::new(InProcessServiceFeature {
            generation: "service:test-v2",
            interface: "interface:test-read-v1",
            service: std::sync::Arc::new(ReadOnlyTestService { value: 4 })
                as std::sync::Arc<dyn ReadOnlyTestServiceContract>,
            publish: true,
            publish_additional: false,
        }));
        assert!(bus.try_finalize().is_err());
        let retained = bus
            .in_process_service::<dyn ReadOnlyTestServiceContract>(&capability, &interface)
            .unwrap()
            .unwrap();
        assert_eq!(retained.generation_id.as_str(), "service:test-v2");
        assert_eq!(retained.service.value(), 2);

        for replacement in [
            InProcessServiceFeature {
                generation: "service:test-v2",
                interface: "interface:test-read-v2",
                service: std::sync::Arc::clone(&second_service),
                publish: true,
                publish_additional: false,
            },
            InProcessServiceFeature {
                generation: "service:test-v2",
                interface: "interface:test-read-v1",
                service: std::sync::Arc::clone(&second_service),
                publish: false,
                publish_additional: false,
            },
            InProcessServiceFeature {
                generation: "service:test-v2",
                interface: "interface:test-read-v1",
                service: std::sync::Arc::clone(&second_service),
                publish: true,
                publish_additional: true,
            },
        ] {
            bus.replace_feature(Box::new(replacement));
            assert!(bus.try_finalize().is_err());
        }

        let empty_service = std::sync::Arc::new(ReadOnlyTestService { value: 0 })
            as std::sync::Arc<dyn ReadOnlyTestServiceContract>;
        let mut empty_bus = EventBus::new();
        empty_bus.register(Box::new(InProcessServiceFeature {
            generation: "service:empty-v1",
            interface: "interface:test-read-v1",
            service: std::sync::Arc::clone(&empty_service),
            publish: false,
            publish_additional: false,
        }));
        empty_bus.try_finalize().unwrap();
        empty_bus.replace_feature(Box::new(InProcessServiceFeature {
            generation: "service:empty-v1",
            interface: "interface:test-read-v1",
            service: empty_service,
            publish: true,
            publish_additional: false,
        }));
        assert!(empty_bus.try_finalize().is_err());
    }

    #[test]
    fn duplicate_internal_binding_is_rejected_without_last_writer_selection() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(NotifierFeature));
        bus.register_internal_tool("hidden", "counter");
        bus.register_internal_tool("hidden", "notifier");

        let error = bus.try_finalize().unwrap_err().to_string();
        assert!(error.contains("graph:duplicate_owner"));
        assert!(error.contains("graph:ambiguous_binding"));
        assert!(!bus.has_registered_tool("hidden"));
    }

    #[test]
    fn duplicate_command_binding_is_rejected_before_publication() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.register(Box::new(AlternateNotifierFeature));

        let error = bus.try_finalize().unwrap_err().to_string();
        assert!(error.contains("graph:duplicate_owner"));
        assert!(error.contains("graph:ambiguous_binding"));
        assert!(bus.command_definitions().is_empty());
    }

    #[test]
    fn human_display_names_are_encoded_as_stable_feature_ids() {
        let mut bus = EventBus::new();
        bus.register(Box::new(DisplayNameFeature("Alpha Plugin / Local")));
        bus.register(Box::new(DisplayNameFeature("Alpha_20Plugin / Local")));
        bus.try_finalize().unwrap();

        let graph = bus.accepted_graph.as_ref().unwrap();
        assert!(
            graph
                .declarations
                .keys()
                .any(|id| id.as_str() == "feature:Alpha_20Plugin_20_2f_20Local")
        );
        assert!(
            graph
                .declarations
                .keys()
                .any(|id| id.as_str() == "feature:Alpha_5f20Plugin_20_2f_20Local")
        );
    }

    #[test]
    fn command_aliases_share_one_canonical_capability_identity() {
        let mut bus = EventBus::new();
        bus.register(Box::new(AliasCommandFeature));
        bus.try_finalize().unwrap();

        let graph = bus.accepted_graph.as_ref().unwrap();
        let canonical = graph
            .invocation_owners
            .get(&(
                omegon_traits::RuntimeInvocationKind::Command,
                "delegate".into(),
            ))
            .unwrap();
        let alias = graph
            .invocation_owners
            .get(&(
                omegon_traits::RuntimeInvocationKind::Command,
                "subagent".into(),
            ))
            .unwrap();
        assert_eq!(canonical, alias);
        assert_eq!(canonical.1.as_str(), "action:delegate");
    }

    #[test]
    fn malformed_external_vocabulary_and_missing_alias_implementations_fail_fallibly() {
        let mut malformed = EventBus::new();
        malformed.register(Box::new(MalformedToolFeature));
        let error = malformed.try_finalize().unwrap_err().to_string();
        assert!(error.contains("invalid tool name"), "{error}");

        let mut missing_alias = EventBus::new();
        missing_alias.register(Box::new(MissingAliasImplementationFeature));
        let error = missing_alias.try_finalize().unwrap_err().to_string();
        assert!(error.contains("parity mismatch"), "{error}");

        let mut malformed_alias = EventBus::new();
        malformed_alias.register(Box::new(MalformedAliasFeature));
        let error = malformed_alias.try_finalize().unwrap_err().to_string();
        assert!(error.contains("invalid canonical alias target"), "{error}");
    }

    #[test]
    fn host_capable_core_tools_are_never_adapted_as_pure() {
        for (name, capability) in [
            (
                crate::tool_registry::core::BASH,
                omegon_traits::ToolCapability::StateChanging,
            ),
            (
                crate::tool_registry::core::PLAN,
                omegon_traits::ToolCapability::Orientation,
            ),
            (
                crate::tool_registry::core::WAIT_FOR_OPERATOR,
                omegon_traits::ToolCapability::ProgressBoundary,
            ),
            (
                crate::tool_registry::core::WHOAMI,
                omegon_traits::ToolCapability::Orientation,
            ),
        ] {
            let definition = ToolDefinition {
                name: name.into(),
                label: name.into(),
                description: name.into(),
                parameters: json!({"type": "object"}),
                capabilities: vec![capability],
            };
            let effects = adapted_tool_effects(&definition);
            assert!(!effects.is_empty(), "{name}");
            assert!(
                effects
                    .iter()
                    .any(|effect| !matches!(effect, omegon_traits::RuntimeEffect::FilesystemRead))
            );
        }
    }

    #[test]
    fn external_tool_declarations_conservatively_include_network_effects() {
        let mut bus = EventBus::new();
        bus.register(Box::new(ExtensionCounterFeature));
        bus.try_finalize().unwrap();

        let graph = bus.accepted_graph.as_ref().unwrap();
        let declaration = graph
            .declarations
            .get(&omegon_traits::RuntimeContributionId::new("feature:recro-coe-agent").unwrap())
            .unwrap();
        assert!(
            declaration.capabilities[0]
                .effects
                .contains(&omegon_traits::RuntimeEffect::NetworkAccess)
        );
    }

    #[test]
    fn register_and_finalize() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        assert_eq!(bus.feature_count(), 2);
        assert_eq!(bus.tool_definitions().len(), 1);
        assert_eq!(bus.command_definitions().len(), 1);
    }

    #[test]
    fn event_delivery_is_sequential() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        bus.emit(&BusEvent::TurnStart { turn: 1 });
        bus.emit(&BusEvent::TurnEnd(Box::new(
            omegon_traits::BusEventTurnEnd {
                turn: 1,
                model: None,
                provider: None,
                estimated_tokens: 0,
                context_window: 200_000,
                context_composition: omegon_traits::ContextComposition::default(),
                actual_input_tokens: 0,
                actual_output_tokens: 0,
                cache_read_tokens: 0,
                provider_telemetry: None,
                dominant_phase: None,
                drift_kind: None,
                progress_signal: omegon_traits::ProgressSignal::None,
            },
        )));

        // Both features should have received both events
        // (Can't inspect directly, but drain_requests would show nothing)
        let requests = bus.drain_requests();
        assert!(requests.is_empty());
    }

    #[test]
    fn requests_accumulated_from_events() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        // No requests from TurnStart
        bus.emit(&BusEvent::TurnStart { turn: 1 });
        assert!(bus.drain_requests().is_empty());

        // SessionEnd triggers a notification request
        bus.emit(&BusEvent::SessionEnd {
            turns: 1,
            tool_calls: 0,
            duration_secs: 10.0,
            initial_prompt: None,
            outcome_summary: None,
        });
        let requests = bus.drain_requests();
        assert_eq!(requests.len(), 1);
        assert!(
            matches!(&requests[0], BusRequest::Notify { message, .. } if message == "Session ended")
        );
    }

    #[test]
    fn command_dispatch() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        let result = bus.dispatch_command("notify", "hello");
        assert!(matches!(result, CommandResult::Display(msg) if msg.contains("hello")));

        let result = bus.dispatch_command("nonexistent", "");
        assert!(matches!(result, CommandResult::NotHandled));
    }

    #[test]
    fn bash_outer_timeout_preserves_explicit_deadlines_and_unbounded_default() {
        assert_eq!(
            effective_tool_timeout("bash", &json!({}), DEFAULT_TOOL_TIMEOUT),
            None
        );
        assert_eq!(
            effective_tool_timeout("bash", &json!({"timeout_secs": 900}), DEFAULT_TOOL_TIMEOUT),
            Some(Duration::from_secs(905))
        );
        assert_eq!(
            effective_tool_timeout("bash", &json!({"timeout": 42}), DEFAULT_TOOL_TIMEOUT),
            Some(Duration::from_secs(47))
        );
    }

    #[test]
    fn non_bash_tools_keep_default_outer_timeout() {
        assert_eq!(
            effective_tool_timeout("count", &json!({}), DEFAULT_TOOL_TIMEOUT),
            Some(DEFAULT_TOOL_TIMEOUT)
        );
    }

    #[tokio::test]
    async fn tool_execution() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 42 }));
        bus.finalize();

        let cancel = tokio_util::sync::CancellationToken::new();
        let result = bus
            .execute_tool("count", "tc1", json!({}), cancel)
            .await
            .unwrap();
        assert_eq!(result.content[0].as_text().unwrap(), "count: 42");
    }

    #[tokio::test]
    async fn unknown_tool_errors() {
        let bus = EventBus::new();
        let cancel = tokio_util::sync::CancellationToken::new();
        let err = bus
            .execute_tool("nonexistent", "tc1", json!({}), cancel)
            .await;
        assert!(err.is_err());
    }

    #[test]
    fn feature_names() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        let names = bus.feature_names();
        assert_eq!(names, vec!["counter", "notifier"]);
    }

    #[test]
    fn publication_rejects_duplicate_tools_without_first_owner_fallback() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(CounterFeature { event_count: 0 }));

        let error = bus.try_finalize().unwrap_err().to_string();
        assert!(error.contains("graph:duplicate_contribution_id"));
        assert!(error.contains("graph:duplicate_owner"));
        assert!(bus.tool_definitions().is_empty());
    }

    #[test]
    fn set_tool_inventory_tracks_finalized_tools() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));

        let inventory = std::sync::Arc::new(std::sync::Mutex::new(
            crate::features::manage_tools::ToolInventorySnapshot::default(),
        ));
        bus.set_tool_inventory(inventory);
        assert!(bus.tool_inventory_names().is_empty());

        bus.finalize();
        assert_eq!(bus.tool_inventory_names(), vec!["count".to_string()]);
        assert_eq!(
            bus.callable_tool_inventory_names(),
            vec!["count".to_string()]
        );
    }

    #[test]
    fn set_tool_inventory_tracks_disabled_tools_as_not_callable() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        let disabled = std::sync::Arc::new(std::sync::Mutex::new(
            crate::features::manage_tools::ToolAdmissionPolicy::default(),
        ));
        bus.set_tool_admission_policy(disabled.clone());
        let inventory = std::sync::Arc::new(std::sync::Mutex::new(
            crate::features::manage_tools::ToolInventorySnapshot::default(),
        ));
        bus.set_tool_inventory(inventory);

        disabled.lock().unwrap().insert("count".to_string());
        bus.finalize();

        assert_eq!(bus.tool_inventory_names(), vec!["count".to_string()]);
        assert!(bus.callable_tool_inventory_names().is_empty());
    }

    #[test]
    fn disabled_tools_filtered_from_definitions() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        // Before disabling: tool is present
        assert_eq!(bus.tool_definitions().len(), 1);
        assert_eq!(bus.all_tool_definitions().len(), 1);

        // Disable the tool
        let disabled = std::sync::Arc::new(std::sync::Mutex::new(
            crate::features::manage_tools::ToolAdmissionPolicy::from_tool_names([
                "count".to_string()
            ]),
        ));
        bus.set_tool_admission_policy(disabled);

        // After disabling: filtered from tool_definitions but still in all_tool_definitions
        assert_eq!(
            bus.tool_definitions().len(),
            0,
            "disabled tool should be filtered"
        );
        assert_eq!(
            bus.all_tool_definitions().len(),
            1,
            "all_tool_definitions should still include it"
        );
    }

    #[test]
    fn disabled_tools_still_executable() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut bus = EventBus::new();
            bus.register(Box::new(CounterFeature { event_count: 0 }));
            bus.finalize();

            let disabled = std::sync::Arc::new(std::sync::Mutex::new(
                crate::features::manage_tools::ToolAdmissionPolicy::from_tool_names([
                    "count".to_string()
                ]),
            ));
            bus.set_tool_admission_policy(disabled);

            // Tool is filtered from definitions...
            assert_eq!(bus.tool_definitions().len(), 0);

            // ...but can still be executed
            let cancel = tokio_util::sync::CancellationToken::new();
            let result = bus.execute_tool("count", "tc1", json!({}), cancel).await;
            assert!(result.is_ok(), "disabled tools must still be executable");
        });
    }

    #[test]
    fn boot_policy_tools_are_not_model_callable_and_cannot_be_reenabled() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        let mutable = std::sync::Arc::new(std::sync::Mutex::new(
            crate::features::manage_tools::ToolAdmissionPolicy::default(),
        ));
        bus.set_tool_admission_policy(mutable.clone());
        bus.set_policy_denied_tools(["count"]);
        bus.finalize();

        assert!(bus.tool_definitions().is_empty());
        assert_eq!(bus.all_tool_definitions().len(), 1);
        mutable.lock().unwrap().remove("count");
        assert!(bus.tool_definitions().is_empty());
        assert!(bus.has_registered_tool("count"));
    }

    #[test]
    fn drain_clears_requests() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        bus.emit(&BusEvent::SessionEnd {
            turns: 1,
            tool_calls: 0,
            duration_secs: 1.0,
            initial_prompt: None,
            outcome_summary: None,
        });
        assert_eq!(bus.drain_requests().len(), 1);
        // Second drain should be empty
        assert!(bus.drain_requests().is_empty());
    }

    #[test]
    fn compact_tool_schema_preserves_property_named_description() {
        let def = ToolDefinition {
            name: "memory_connect".into(),
            label: "Connect".into(),
            description: "Connect facts".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["relation", "description"],
                "properties": {
                    "relation": { "type": "string", "description": "Edge type" },
                    "description": { "type": "string", "description": "Edge rationale" }
                }
            }),
            capabilities: vec![],
        };

        let compact = compact_tool_schema(&def);
        let properties = compact.parameters["properties"].as_object().unwrap();
        assert!(properties.contains_key("description"));
        assert_eq!(properties["description"]["type"], "string");
        assert!(
            !properties["description"]
                .as_object()
                .unwrap()
                .contains_key("description")
        );
        assert_eq!(
            compact.parameters["required"],
            serde_json::json!(["relation", "description"])
        );
    }

    #[test]
    fn compact_tool_schema_strips_descriptions() {
        let def = ToolDefinition {
            name: "test_tool".into(),
            label: "Test".into(),
            description: "A test tool that does things".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to read"
                    },
                    "limit": {
                        "type": "number",
                        "description": "Maximum number of lines to return",
                        "default": 100
                    }
                },
                "required": ["path"]
            }),
            capabilities: vec![],
        };

        let compact = compact_tool_schema(&def);

        // Top-level description preserved
        assert_eq!(compact.description, "A test tool that does things");

        // Parameter descriptions stripped
        let params = compact.parameters.as_object().unwrap();
        let props = params["properties"].as_object().unwrap();
        assert!(
            !props["path"]
                .as_object()
                .unwrap()
                .contains_key("description")
        );
        assert!(
            !props["limit"]
                .as_object()
                .unwrap()
                .contains_key("description")
        );

        // Structural info preserved
        assert_eq!(props["path"]["type"], "string");
        assert_eq!(props["limit"]["type"], "number");
        assert_eq!(props["limit"]["default"], 100);
        assert_eq!(params["required"][0], "path");

        // Compact schema is smaller
        let full_size = serde_json::to_string(&def.parameters).unwrap().len();
        let compact_size = serde_json::to_string(&compact.parameters).unwrap().len();
        assert!(
            compact_size < full_size,
            "compact ({compact_size}) should be smaller than full ({full_size})"
        );
    }

    // ─── Regression: slim mode tool filtering ───────────────────────────

    /// Helper: register N dummy tools with given names.
    fn bus_with_tools(names: &[&str]) -> EventBus {
        struct MultiToolFeature {
            tools: Vec<ToolDefinition>,
        }

        #[async_trait]
        impl Feature for MultiToolFeature {
            fn name(&self) -> &str {
                "multi"
            }
            fn tools(&self) -> Vec<ToolDefinition> {
                self.tools.clone()
            }
            async fn execute(
                &self,
                _: &str,
                _: &str,
                _: serde_json::Value,
                _: tokio_util::sync::CancellationToken,
            ) -> anyhow::Result<ToolResult> {
                Ok(ToolResult {
                    content: vec![],
                    details: json!(null),
                })
            }
        }

        let tools = names
            .iter()
            .map(|name| ToolDefinition {
                name: name.to_string(),
                label: name.to_string(),
                description: String::new(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![],
            })
            .collect();
        let disabled = std::sync::Arc::new(std::sync::Mutex::new(
            crate::features::manage_tools::ToolAdmissionPolicy::default(),
        ));
        let mut bus = EventBus::new();
        bus.register(Box::new(MultiToolFeature { tools }));
        bus.finalize();
        bus.set_tool_admission_policy(disabled);
        bus
    }

    #[test]
    fn base_defaults_disable_situational_tools() {
        use crate::tool_registry as reg;

        // Tools disabled by default in ALL modes (base defaults)
        let base_disabled = [
            reg::persona::SWITCH_PERSONA,
            reg::harness_settings::HARNESS_SETTINGS,
            reg::session_log::SESSION_LOG,
            reg::lifecycle::OPENSPEC_MANAGE,
            reg::auth::AUTH_STATUS,
        ];

        // Tools that stay enabled in Full (non-slim) mode
        let full_enabled = [
            "bash",
            "read",
            reg::delegate::DELEGATE,
            reg::cleave::CLEAVE_RUN,
        ];

        let all_names: Vec<&str> = base_disabled
            .iter()
            .copied()
            .chain(full_enabled.iter().copied())
            .collect();

        // Non-slim: base defaults applied, delegation/cleave stay enabled
        let mut bus = bus_with_tools(&all_names);
        bus.apply_operator_tool_profile(false, &[], &[]);
        let defs = bus.tool_definitions();
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        for tool in &base_disabled {
            assert!(
                !def_names.contains(tool),
                "'{tool}' must be disabled by base defaults"
            );
        }
        assert!(
            def_names.contains(&reg::delegate::DELEGATE),
            "delegate stays enabled in Full mode"
        );
        assert!(
            def_names.contains(&reg::cleave::CLEAVE_RUN),
            "cleave stays enabled in Full mode"
        );
    }

    #[test]
    fn slim_mode_additionally_disables_delegation_and_orchestration() {
        use crate::tool_registry as reg;

        // Tools additionally disabled in slim mode (on top of base defaults)
        let slim_only_disabled = [
            reg::delegate::DELEGATE,
            reg::delegate::DELEGATE_RESULT,
            reg::cleave::CLEAVE_RUN,
            reg::cleave::CLEAVE_ASSESS,
        ];

        let always_enabled = ["bash", "read", "write", "edit", "commit"];

        let all_names: Vec<&str> = slim_only_disabled
            .iter()
            .copied()
            .chain(always_enabled.iter().copied())
            .collect();

        let mut bus = bus_with_tools(&all_names);
        bus.apply_operator_tool_profile(true, &[], &[]);

        let defs = bus.tool_definitions();
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        for tool in &always_enabled {
            assert!(
                def_names.contains(tool),
                "core tool '{tool}' must remain enabled in slim mode"
            );
        }
        for tool in &slim_only_disabled {
            assert!(
                !def_names.contains(tool),
                "'{tool}' must be disabled in slim mode"
            );
        }
    }

    #[test]
    fn lazy_tool_surface_keeps_dynamic_extension_tools_visible_after_turn_one() {
        let bus = bus_with_tools(&["bash", "reader_doctor", "reader_open"]);
        let used_tools = std::collections::HashSet::new();

        let defs = bus.tool_definitions_lazy(false, 2, &used_tools);
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert!(def_names.contains(&"bash"));
        assert!(
            def_names.contains(&"reader_doctor"),
            "dynamic native extension tools must stay visible after turn 1"
        );
        assert!(
            def_names.contains(&"reader_open"),
            "dynamic native extension tools must stay visible after turn 1"
        );
    }

    #[test]
    fn lazy_tool_surface_still_hides_unused_static_non_core_tools_after_turn_one() {
        use crate::tool_registry as reg;

        let bus = bus_with_tools(&["bash", reg::web_search::WEB_SEARCH]);
        let used_tools = std::collections::HashSet::new();

        let defs = bus.tool_definitions_lazy(false, 2, &used_tools);
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert!(def_names.contains(&"bash"));
        assert!(
            !def_names.contains(&reg::web_search::WEB_SEARCH),
            "unused static non-core tools should remain lazy-filtered"
        );
    }

    #[test]
    fn posture_whitelist_restricts_to_listed_tools_only() {
        let mut bus = bus_with_tools(&["bash", "read", "write", "edit", "delegate"]);
        let enabled = vec!["bash".to_string(), "read".to_string()];
        bus.apply_operator_tool_profile(false, &[], &enabled);

        let defs = bus.tool_definitions();
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert_eq!(def_names.len(), 2);
        assert!(def_names.contains(&"bash"));
        assert!(def_names.contains(&"read"));
        assert!(!def_names.contains(&"write"));
        assert!(!def_names.contains(&"delegate"));
    }
}
