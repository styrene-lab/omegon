//! Request-scoped provider routing and durable route leases.

use std::fs::{self, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bridge::{LlmBridge, LlmEvent, LlmMessage, StreamOptions};
use crate::conversation::{AssistantMessage, ToolCall};
use crate::session_authority::{RouteLeaseRecorded, SessionAuthorityHandle};
use crate::upstream_errors::{
    TransientFailureKind, UpstreamFailureLogEntry, append_upstream_failure_log,
    classify_upstream_error_for_provider, is_context_overflow, is_malformed_history,
};

const ROUTE_LEASE_SCHEMA_VERSION: u16 = 2;
const TOOL_SCHEMA_NORMALIZER_ID: &str = "system:tool-schema-normalizer";
const TOOL_SCHEMA_NORMALIZER_GENERATION: &str = "tool-schema-normalizer:builtin-v1";
const MANIFEST_CHAT_COMPLETIONS_GENERATION: &str = "provider:manifest-chat-completions:builtin-v1";

fn tool_schema_normalizer_identity() -> (
    omegon_traits::RuntimeContributionId,
    omegon_traits::RuntimeContributionGenerationId,
) {
    (
        omegon_traits::RuntimeContributionId::new(TOOL_SCHEMA_NORMALIZER_ID)
            .expect("built-in schema normalizer ID is valid"),
        omegon_traits::RuntimeContributionGenerationId::new(TOOL_SCHEMA_NORMALIZER_GENERATION)
            .expect("built-in schema normalizer generation is valid"),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderRouteLease {
    pub(crate) schema_version: u16,
    pub(crate) lease_id: Uuid,
    pub(crate) request_id: Uuid,
    pub(crate) selected_provider_id: String,
    pub(crate) selected_model_id: String,
    pub(crate) serving_provider_id: String,
    pub(crate) serving_model_id: String,
    pub(crate) schema_dialect: String,
    pub(crate) credential_source_class: String,
    pub(crate) fallback_reason: Option<String>,
    pub(crate) contribution_generation_id: String,
    pub(crate) route_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) endpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) adapter_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) inventory_generation: Option<u64>,
}

impl ProviderRouteLease {
    fn for_turn(&self, turn_id: Uuid) -> RouteLeaseRecorded {
        RouteLeaseRecorded {
            lease_id: self.lease_id,
            request_id: self.request_id,
            turn_id,
            selected_provider_id: self.selected_provider_id.clone(),
            selected_model_id: self.selected_model_id.clone(),
            serving_provider_id: self.serving_provider_id.clone(),
            serving_model_id: self.serving_model_id.clone(),
            schema_dialect: self.schema_dialect.clone(),
            credential_source_class: self.credential_source_class.clone(),
            fallback_reason: self.fallback_reason.clone(),
            contribution_generation_id: self.contribution_generation_id.clone(),
            route_policy: self.route_policy.clone(),
        }
    }

    fn validate_current_generation(&self) -> anyhow::Result<()> {
        if let (Some(endpoint_id), Some(adapter_id), Some(_)) = (
            self.endpoint_id.as_deref(),
            self.adapter_id.as_deref(),
            self.inventory_generation,
        ) {
            if endpoint_id.is_empty()
                || adapter_id != crate::inference_inventory::AdapterId::CHAT_COMPLETIONS
                || self.contribution_generation_id != MANIFEST_CHAT_COMPLETIONS_GENERATION
                || self.schema_dialect != "open_ai"
                || self.fallback_reason.is_some()
            {
                anyhow::bail!("manifest provider route lease semantics are invalid");
            }
            return Ok(());
        }
        if self.endpoint_id.is_some()
            || self.adapter_id.is_some()
            || self.inventory_generation.is_some()
        {
            anyhow::bail!("manifest provider route lease provenance is incomplete");
        }
        let contribution = crate::provider_contributions::registry()
            .get(&self.serving_provider_id)
            .ok_or_else(|| anyhow::anyhow!("serving provider contribution is absent"))?;
        if contribution.owner_generation_id.as_str() != self.contribution_generation_id {
            anyhow::bail!("provider route lease contribution generation is stale");
        }
        if contribution.tools.dialect_name() != self.schema_dialect
            || self.credential_source_class.trim().is_empty()
        {
            anyhow::bail!("provider route lease semantics do not match its contribution");
        }
        if self.selected_provider_id != self.serving_provider_id
            && !crate::provider_contributions::registry()
                .fallback_targets(&self.selected_provider_id, &self.selected_model_id)
                .any(|provider| provider == self.serving_provider_id.as_str())
        {
            anyhow::bail!("provider route lease fallback is not declared compatible");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepRouteLeaseFact {
    schema_version: u16,
    step_id: Uuid,
    recorded_at: String,
    lease: ProviderRouteLease,
}

#[derive(Debug, Clone)]
pub(crate) struct StepRouteLeaseRecorder {
    step_id: Uuid,
    log_path: PathBuf,
}

impl StepRouteLeaseRecorder {
    pub(crate) fn for_ephemeral_step(step_id: Uuid) -> anyhow::Result<Self> {
        #[cfg(test)]
        let home = std::env::temp_dir().join(format!("omegon-route-tests-{}", std::process::id()));
        #[cfg(not(test))]
        let home = crate::paths::omegon_home()?;
        Ok(Self::at_path(
            step_id,
            home.join("runtime").join("route-leases.jsonl"),
        ))
    }

    fn at_path(step_id: Uuid, log_path: PathBuf) -> Self {
        Self { step_id, log_path }
    }

    fn record(&self, lease: &ProviderRouteLease) -> anyhow::Result<()> {
        if let Some(parent) = self.log_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock_path = self.log_path.with_extension("jsonl.lock");
        let _guard = crate::filelock::acquire_lock(&lock_path)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let fact = StepRouteLeaseFact {
            schema_version: ROUTE_LEASE_SCHEMA_VERSION,
            step_id: self.step_id,
            recorded_at: recorded_at_now(),
            lease: lease.clone(),
        };
        let mut encoded = serde_json::to_vec(&fact)?;
        encoded.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DurableRouteIdentity {
    pub(crate) lease_id: Uuid,
    pub(crate) request_id: Uuid,
}

pub(crate) enum RouteLeaseOwner<'a> {
    Session {
        authority: &'a SessionAuthorityHandle,
        turn_id: Uuid,
    },
    Step(&'a StepRouteLeaseRecorder),
}

impl RouteLeaseOwner<'_> {
    fn record(&self, lease: &ProviderRouteLease) -> anyhow::Result<()> {
        lease.validate_current_generation()?;
        match self {
            Self::Session { authority, turn_id } => {
                let recorded_at = recorded_at_now();
                authority
                    .record_route_lease(&recorded_at, lease.for_turn(*turn_id))
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                if let (Some(endpoint_id), Some(adapter_id), Some(inventory_generation)) = (
                    lease.endpoint_id.as_ref(),
                    lease.adapter_id.as_ref(),
                    lease.inventory_generation,
                ) {
                    authority
                        .record_route_endpoint_provenance(
                            &recorded_at,
                            crate::session_authority::RouteEndpointProvenanceRecorded {
                                lease_id: lease.lease_id,
                                endpoint_id: endpoint_id.clone(),
                                adapter_id: adapter_id.clone(),
                                inventory_generation,
                            },
                        )
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                }
                Ok(())
            }
            Self::Step(recorder) => recorder.record(lease),
        }
    }
}

pub(crate) fn record_loop_route_lease(
    scope: &crate::invocation_service::InvocationScope,
    step_id: Uuid,
    selected_model: &str,
    serving_model: &str,
    credential_source_class: Option<&str>,
) -> anyhow::Result<()> {
    record_loop_route_lease_for_request(
        scope,
        step_id,
        selected_model,
        serving_model,
        credential_source_class,
        None,
        None,
    )
    .map(|_| ())
}

fn record_loop_route_lease_for_request(
    scope: &crate::invocation_service::InvocationScope,
    step_id: Uuid,
    selected_model: &str,
    serving_model: &str,
    credential_source_class: Option<&str>,
    semantic_request: Option<&crate::loop_driver::LoopModelRequestIdentity>,
    endpoint_provenance: Option<&crate::bridge::EndpointRouteProvenance>,
) -> anyhow::Result<Option<DurableRouteIdentity>> {
    let lease = route_lease(
        selected_model,
        serving_model,
        credential_source_class,
        semantic_request.map(|request| request.request_id),
        endpoint_provenance,
    )?;
    if let (Some(authority), Some(turn_id)) = (scope.authority.as_ref(), scope.turn_id) {
        RouteLeaseOwner::Session { authority, turn_id }.record(&lease)?;
        if let Some(request) = semantic_request {
            if request.turn_id != turn_id || request.request_id != lease.request_id {
                anyhow::bail!("semantic request identity contradicts provider route lease");
            }
            authority
                .join_model_request_route(
                    Uuid::new_v4(),
                    &recorded_at_now(),
                    crate::session_authority::ModelRequestRouteJoined {
                        request_id: request.request_id,
                        step_id: request.step_id,
                        turn_id: request.turn_id,
                        lease_id: lease.lease_id,
                    },
                )
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            return Ok(Some(DurableRouteIdentity {
                lease_id: lease.lease_id,
                request_id: lease.request_id,
            }));
        }
        return Ok(None);
    }
    if scope.authority.is_some() || scope.turn_id.is_some() || scope.session_id.is_some() {
        anyhow::bail!("incomplete session authority cannot own a provider route lease");
    }
    if semantic_request.is_some() {
        anyhow::bail!("sessionless route cannot join a durable model request");
    }
    StepRouteLeaseRecorder::for_ephemeral_step(step_id)?.record(&lease)?;
    Ok(None)
}

#[cfg(test)]
pub(crate) fn record_loop_route_lease_for_test(
    scope: &crate::invocation_service::InvocationScope,
    step_id: Uuid,
    selected_model: &str,
    serving_model: &str,
    request: &crate::loop_driver::LoopModelRequestIdentity,
) -> anyhow::Result<()> {
    record_loop_route_lease_for_request(
        scope,
        step_id,
        selected_model,
        serving_model,
        Some("test"),
        Some(request),
        None,
    )
    .map(|_| ())
}

pub(crate) struct ResolvedProviderRoute {
    selected_model: String,
    serving_model: String,
    credential_source_class: String,
    native_model: String,
    endpoint_provenance: Option<crate::bridge::EndpointRouteProvenance>,
    admitted_capabilities: Option<AdmittedModelCapabilities>,
    bridge: Box<dyn LlmBridge>,
}

struct RoutedBridge {
    selected_model: String,
    serving_model: String,
    credential_source_class: String,
    native_model: String,
    endpoint_provenance: Option<crate::bridge::EndpointRouteProvenance>,
    admitted_capabilities: Option<AdmittedModelCapabilities>,
    inner: Box<dyn LlmBridge>,
}

#[async_trait::async_trait]
impl LlmBridge for RoutedBridge {
    fn validate_request_capabilities(
        &self,
        tools: &[omegon_traits::ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<()> {
        validate_admitted_request(self.admitted_capabilities.as_ref(), tools, options)?;
        self.inner.validate_request_capabilities(tools, options)
    }

    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[omegon_traits::ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
        self.validate_request_capabilities(tools, options)?;
        let mut native_options = options.clone();
        native_options.model = Some(self.native_model.clone());
        self.inner
            .stream(system_prompt, messages, tools, &native_options)
            .await
    }

    fn serving_model_hint(&self) -> Option<&str> {
        Some(&self.serving_model)
    }

    fn selected_model_hint(&self) -> Option<&str> {
        Some(&self.selected_model)
    }

    fn native_model_hint(&self) -> Option<&str> {
        Some(&self.native_model)
    }

    fn endpoint_route_provenance_hint(&self) -> Option<&crate::bridge::EndpointRouteProvenance> {
        self.endpoint_provenance.as_ref()
    }

    fn credential_source_class_hint(&self) -> Option<&str> {
        Some(&self.credential_source_class)
    }

    async fn shutdown(&self) {
        self.inner.shutdown().await;
    }
}

impl ResolvedProviderRoute {
    pub(crate) fn serving_model(&self) -> &str {
        &self.serving_model
    }

    pub(crate) fn into_unleased_bridge(self) -> Box<dyn LlmBridge> {
        Box::new(RoutedBridge {
            selected_model: self.selected_model,
            serving_model: self.serving_model,
            credential_source_class: self.credential_source_class,
            native_model: self.native_model,
            endpoint_provenance: self.endpoint_provenance,
            admitted_capabilities: self.admitted_capabilities,
            inner: self.bridge,
        })
    }

    pub(crate) async fn stream(
        &self,
        owner: RouteLeaseOwner<'_>,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[omegon_traits::ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
        validate_admitted_request(self.admitted_capabilities.as_ref(), tools, options)?;
        self.bridge.validate_request_capabilities(tools, options)?;
        let lease = route_lease(
            &self.selected_model,
            &self.native_model,
            Some(&self.credential_source_class),
            None,
            self.endpoint_provenance.as_ref(),
        )?;
        owner.record(&lease)?;
        let mut native_options = options.clone();
        native_options.model = Some(self.native_model.clone());
        self.bridge
            .stream(system_prompt, messages, tools, &native_options)
            .await
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ProviderRouteService;

#[async_trait::async_trait]
pub(crate) trait ProviderRouteServiceContract: Send + Sync {
    async fn resolve(
        &self,
        _model_spec: &str,
        _secrets: Option<&omegon_secrets::SecretsManager>,
    ) -> Option<ResolvedProviderRoute> {
        None
    }
    async fn resolve_exact_admitted(
        &self,
        _model_spec: &str,
        _secrets: Option<&omegon_secrets::SecretsManager>,
        _inventory: &crate::inference_inventory::InventorySnapshot,
        _required_capabilities: &[String],
    ) -> Option<ResolvedProviderRoute> {
        None
    }
    async fn startup_route(
        &self,
        bridge: &dyn LlmBridge,
        setup: &crate::loop_driver::LoopRouteSetup,
        policy: &LoopRoutePolicy,
    ) -> LoopRoute;
    async fn turn_route(
        &self,
        bridge: &dyn LlmBridge,
        setup: &crate::loop_driver::LoopRouteSetup,
        policy: &LoopRoutePolicy,
        base: &StreamOptions,
    ) -> LoopRoute;
    async fn prepare(
        &self,
        route: &LoopRoute,
        setup: &crate::loop_driver::LoopRouteSetup,
        events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    );
    async fn dispatch(
        &self,
        bridge: &dyn LlmBridge,
        request: LoopRouteRequest<'_>,
    ) -> anyhow::Result<LoopRouteDispatch>;
    async fn compact(
        &self,
        bridge: &dyn LlmBridge,
        request: LoopCompactionRequest<'_>,
    ) -> anyhow::Result<String>;
    fn stop_notice(&self, route: &LoopRoute, raw: &serde_json::Value)
    -> Option<ProviderStopNotice>;
    fn failure_kind(&self, error: &anyhow::Error) -> LoopRouteFailure;
    fn provider_id(&self, model: &str) -> String;
    fn canonical_model_spec(&self, model: &str) -> String;
}

#[async_trait::async_trait]
impl ProviderRouteServiceContract for ProviderRouteService {
    async fn resolve(
        &self,
        model_spec: &str,
        secrets: Option<&omegon_secrets::SecretsManager>,
    ) -> Option<ResolvedProviderRoute> {
        resolve_provider_route(model_spec, secrets, false).await
    }

    async fn resolve_exact_admitted(
        &self,
        model_spec: &str,
        secrets: Option<&omegon_secrets::SecretsManager>,
        inventory: &crate::inference_inventory::InventorySnapshot,
        required_capabilities: &[String],
    ) -> Option<ResolvedProviderRoute> {
        let offering = match admit_exact_route(inventory, model_spec, required_capabilities) {
            Ok(offering) => offering,
            Err(rejection) => {
                tracing::warn!(
                    model_spec,
                    ?rejection,
                    "exact provider route rejected by active inventory"
                );
                return None;
            }
        };
        let endpoint = inventory.endpoints.get(&offering.endpoint.value)?;
        // Embedded endpoint URLs also serve discovery. They do not turn a
        // compiled provider's OAuth/API-key factory into a custom manifest.
        // Only canonical identities with untouched routing declarations may use
        // that factory; user-owned routes retain manifest secret binding.
        let canonical_identity = offering
            .id
            .0
            .split_once(':')
            .is_some_and(|(provider, model)| {
                endpoint.id.0 == provider
                    && offering.native_model_id.value == model
                    && crate::provider_contributions::registry()
                        .get(provider)
                        .is_some_and(|entry| entry.executable)
            });
        let embedded_native = canonical_identity
            && [
                endpoint.transport.source,
                endpoint.adapter.source,
                endpoint.secret_refs.source,
            ]
            .into_iter()
            .all(|source| source == crate::inference_inventory::InventorySource::Embedded)
            && [offering.endpoint.source, offering.native_model_id.source]
                .into_iter()
                .all(|source| {
                    matches!(
                        source,
                        crate::inference_inventory::InventorySource::Embedded
                            | crate::inference_inventory::InventorySource::Discovery
                    )
                });
        if !embedded_native {
            let plan =
                match admit_manifest_endpoint_route(inventory, model_spec, required_capabilities) {
                    Ok(plan) => plan,
                    Err(error) => {
                        tracing::warn!(model_spec, %error, "manifest endpoint route rejected");
                        return None;
                    }
                };
            let api_key = if let Some(secrets) = secrets {
                secrets.resolve_async(&plan.secret_ref).await
            } else {
                std::env::var(&plan.secret_ref)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            }?;
            return Some(construct_manifest_endpoint_route(plan, api_key));
        }
        let mut route = resolve_provider_route(model_spec, secrets, true).await?;
        let provider_id = crate::providers::infer_provider_id(&route.serving_model);
        let provider_supports_tools = crate::provider_contributions::registry()
            .get(&provider_id)
            .is_some_and(|contribution| {
                matches!(
                    contribution.tools,
                    crate::provider_contributions::ProviderToolContract::Supported(_)
                )
            });
        route.admitted_capabilities = Some(AdmittedModelCapabilities {
            tools: offering_capability_admission(
                offering,
                "tools",
                crate::inference_inventory::EvidenceKind::Declared,
            ),
            reasoning: offering_capability_admission(
                offering,
                "reasoning",
                crate::inference_inventory::EvidenceKind::Declared,
            ),
            provider_supports_tools,
        });
        Some(route)
    }

    async fn startup_route(
        &self,
        bridge: &dyn LlmBridge,
        setup: &crate::loop_driver::LoopRouteSetup,
        policy: &LoopRoutePolicy,
    ) -> LoopRoute {
        loop_startup_route(bridge, setup, policy).await
    }

    async fn turn_route(
        &self,
        bridge: &dyn LlmBridge,
        setup: &crate::loop_driver::LoopRouteSetup,
        policy: &LoopRoutePolicy,
        base: &StreamOptions,
    ) -> LoopRoute {
        loop_turn_route(bridge, setup, policy, base).await
    }

    async fn prepare(
        &self,
        route: &LoopRoute,
        setup: &crate::loop_driver::LoopRouteSetup,
        events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    ) {
        let loop_route = crate::loop_driver::LoopRoute {
            selected_model: route.selected_model.clone(),
            serving_model: route.serving_model.clone(),
            provider_id: route.provider_id.clone(),
            schema_dialect: route.schema_dialect.clone(),
            contribution_generation_id: route.contribution_generation_id.clone(),
            normalizer_contribution_id: route.normalizer_contribution_id.clone(),
            normalizer_generation_id: route.normalizer_generation_id.clone(),
        };
        if !setup.prepare(&loop_route, events).await {
            prepare_loop_route(route, events, None).await;
        }
    }

    async fn dispatch(
        &self,
        bridge: &dyn LlmBridge,
        request: LoopRouteRequest<'_>,
    ) -> anyhow::Result<LoopRouteDispatch> {
        dispatch_loop_route(bridge, request).await
    }

    async fn compact(
        &self,
        bridge: &dyn LlmBridge,
        request: LoopCompactionRequest<'_>,
    ) -> anyhow::Result<String> {
        compact_loop_route(bridge, request).await
    }

    fn stop_notice(
        &self,
        route: &LoopRoute,
        raw: &serde_json::Value,
    ) -> Option<ProviderStopNotice> {
        provider_stop_notice(route, raw)
    }

    fn failure_kind(&self, error: &anyhow::Error) -> LoopRouteFailure {
        classify_loop_route_failure(error)
    }

    fn provider_id(&self, model: &str) -> String {
        crate::providers::infer_provider_id(model)
    }

    fn canonical_model_spec(&self, model: &str) -> String {
        crate::providers::canonical_model_spec(model)
    }
}

impl ProviderRouteService {
    pub(crate) async fn resolve(
        self,
        model_spec: &str,
        secrets: Option<&omegon_secrets::SecretsManager>,
    ) -> Option<ResolvedProviderRoute> {
        resolve_provider_route(model_spec, secrets, false).await
    }
}

pub(crate) fn admit_exact_route<'a>(
    snapshot: &'a crate::inference_inventory::InventorySnapshot,
    model_spec: &str,
    required_capabilities: &[String],
) -> Result<
    &'a crate::inference_inventory::InferenceOffering,
    crate::inference_inventory::ExactAdmissionRejection,
> {
    use crate::inference_inventory::{CompatibilityRequest, ExactAdmissionRejection, OfferingId};

    let requested_id = OfferingId(crate::inference_runtime::normalize_route_id_for_resolution(
        model_spec,
    ));
    let offering_id = if snapshot.offerings.contains_key(&requested_id) {
        requested_id
    } else {
        let canonical = crate::providers::canonical_model_spec(model_spec);
        let Some(provider) = crate::providers::infer_provider_id_strict(&canonical) else {
            return Err(ExactAdmissionRejection::UnknownOffering(OfferingId(
                canonical,
            )));
        };
        OfferingId(crate::inference_runtime::normalize_route_id_for_resolution(
            &format!(
                "{provider}:{}",
                crate::providers::model_id_from_spec(&canonical)
            ),
        ))
    };
    let offering = snapshot
        .offerings
        .get(&offering_id)
        .ok_or_else(|| ExactAdmissionRejection::UnknownOffering(offering_id.clone()))?;
    let interface = snapshot
        .endpoints
        .get(&offering.endpoint.value)
        .map(|endpoint| endpoint.adapter.value.clone())
        .ok_or_else(|| ExactAdmissionRejection::UnknownOffering(offering_id.clone()))?;
    let request = CompatibilityRequest {
        interface,
        required_capabilities: required_capabilities.iter().cloned().collect(),
        exact_offering: Some(offering_id),
        ..Default::default()
    };
    snapshot.admit_exact(&request)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityAdmission {
    Supported,
    Missing,
    InsufficientEvidence,
}

#[derive(Debug, Clone, Copy)]
struct AdmittedModelCapabilities {
    tools: CapabilityAdmission,
    reasoning: CapabilityAdmission,
    provider_supports_tools: bool,
}

fn offering_capability_admission(
    offering: &crate::inference_inventory::InferenceOffering,
    capability: &str,
    minimum_evidence: crate::inference_inventory::EvidenceKind,
) -> CapabilityAdmission {
    match offering.capabilities.get(capability) {
        Some(value) if value.value && value.evidence >= minimum_evidence => {
            CapabilityAdmission::Supported
        }
        Some(value) if value.value => CapabilityAdmission::InsufficientEvidence,
        _ => CapabilityAdmission::Missing,
    }
}

#[derive(Debug, Clone)]
struct AdmittedManifestEndpointRoute {
    selected_offering_id: String,
    selected_provider_id: String,
    selected_model_id: String,
    native_model_id: String,
    endpoint_id: String,
    inventory_generation: u64,
    adapter_id: String,
    base_url: String,
    secret_ref: String,
    admitted_capabilities: AdmittedModelCapabilities,
}

#[derive(Debug, thiserror::Error)]
enum ManifestEndpointAdmissionError {
    #[error("inventory admission rejected: {0}")]
    Inventory(String),
    #[error("manifest endpoint adapter '{0}' is not executable")]
    UnsupportedAdapter(String),
    #[error("manifest endpoint transport is not executable for this adapter")]
    UnsupportedTransport,
    #[error("manifest endpoint HTTP base URL is invalid")]
    InvalidBaseUrl,
    #[error("manifest endpoint HTTP base URL must use HTTPS or a loopback host")]
    InsecureBaseUrl,
    #[error("manifest endpoint must declare exactly one bearer-token secret reference")]
    InvalidSecretConfiguration,
    #[error("manifest endpoint secret reference must be '{expected}'")]
    UnboundSecretReference { expected: String },
    #[error("manifest endpoint adapter, transport, and secret declaration must have one owner")]
    MixedEndpointOwnership,
}

fn manifest_endpoint_secret_name(
    source: crate::inference_inventory::InventorySource,
    endpoint_id: &str,
) -> String {
    use crate::inference_inventory::InventorySource;
    let source = match source {
        InventorySource::Embedded => "EMBEDDED",
        InventorySource::Organization => "ORGANIZATION",
        InventorySource::User => "USER",
        InventorySource::Project => "PROJECT",
        InventorySource::Session => "SESSION",
        InventorySource::Discovery => "DISCOVERY",
        InventorySource::Probe => "PROBE",
    };
    let encoded_id = endpoint_id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    format!("OMEGON_{source}_ENDPOINT_{encoded_id}_TOKEN")
}

fn admit_manifest_endpoint_route(
    snapshot: &crate::inference_inventory::InventorySnapshot,
    model_spec: &str,
    required_capabilities: &[String],
) -> Result<AdmittedManifestEndpointRoute, ManifestEndpointAdmissionError> {
    use crate::inference_inventory::{AdapterId, TransportSpec};
    let offering = admit_exact_route(snapshot, model_spec, required_capabilities)
        .map_err(|error| ManifestEndpointAdmissionError::Inventory(error.to_string()))?;
    let endpoint = snapshot
        .endpoints
        .get(&offering.endpoint.value)
        .ok_or(ManifestEndpointAdmissionError::UnsupportedTransport)?;
    if endpoint.adapter.value.0 != AdapterId::CHAT_COMPLETIONS {
        return Err(ManifestEndpointAdmissionError::UnsupportedAdapter(
            endpoint.adapter.value.0.clone(),
        ));
    }
    let TransportSpec::Http { base_url } = &endpoint.transport.value else {
        return Err(ManifestEndpointAdmissionError::UnsupportedTransport);
    };
    let parsed = url::Url::parse(base_url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .filter(|url| {
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
        })
        .ok_or(ManifestEndpointAdmissionError::InvalidBaseUrl)?;
    let loopback = match parsed.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    if parsed.scheme() != "https" && !loopback {
        return Err(ManifestEndpointAdmissionError::InsecureBaseUrl);
    }
    let [secret_ref] = endpoint.secret_refs.value.as_slice() else {
        return Err(ManifestEndpointAdmissionError::InvalidSecretConfiguration);
    };
    let owner = endpoint.secret_refs.source;
    if endpoint.adapter.source != owner || endpoint.transport.source != owner {
        return Err(ManifestEndpointAdmissionError::MixedEndpointOwnership);
    }
    let expected_secret_ref = manifest_endpoint_secret_name(owner, &endpoint.id.0);
    if secret_ref != &expected_secret_ref {
        return Err(ManifestEndpointAdmissionError::UnboundSecretReference {
            expected: expected_secret_ref,
        });
    }
    let (selected_provider_id, selected_model_id) = offering
        .id
        .0
        .split_once(':')
        .map(|(provider, model)| (provider.to_string(), model.to_string()))
        .unwrap_or_else(|| (offering.endpoint.value.0.clone(), offering.id.0.clone()));
    Ok(AdmittedManifestEndpointRoute {
        selected_offering_id: offering.id.0.clone(),
        selected_provider_id,
        selected_model_id,
        native_model_id: offering.native_model_id.value.clone(),
        endpoint_id: endpoint.id.0.clone(),
        inventory_generation: snapshot.generation,
        adapter_id: endpoint.adapter.value.0.clone(),
        base_url: parsed.as_str().trim_end_matches('/').to_string(),
        secret_ref: secret_ref.clone(),
        admitted_capabilities: AdmittedModelCapabilities {
            tools: offering_capability_admission(
                offering,
                "tools",
                crate::inference_inventory::EvidenceKind::Declared,
            ),
            reasoning: offering_capability_admission(
                offering,
                "reasoning",
                crate::inference_inventory::EvidenceKind::Declared,
            ),
            provider_supports_tools: true,
        },
    })
}

fn construct_manifest_endpoint_route(
    plan: AdmittedManifestEndpointRoute,
    api_key: String,
) -> ResolvedProviderRoute {
    let native_model = format!("{}:{}", plan.selected_provider_id, plan.native_model_id);
    ResolvedProviderRoute {
        selected_model: plan.selected_offering_id.clone(),
        serving_model: plan.selected_offering_id,
        credential_source_class: "declared_bearer_secret".into(),
        native_model,
        endpoint_provenance: Some(crate::bridge::EndpointRouteProvenance {
            selected_provider_id: plan.selected_provider_id.clone(),
            endpoint_id: plan.endpoint_id,
            adapter_id: plan.adapter_id,
            inventory_generation: plan.inventory_generation,
            contribution_generation_id: MANIFEST_CHAT_COMPLETIONS_GENERATION.into(),
            schema_dialect: "open_ai".into(),
        }),
        admitted_capabilities: Some(plan.admitted_capabilities),
        bridge: Box::new(crate::providers::OpenAICompatClient::new_manifest(
            api_key,
            plan.base_url,
            plan.selected_provider_id,
        )),
    }
}

fn validate_capability(capability: CapabilityAdmission, name: &str) -> anyhow::Result<()> {
    match capability {
        CapabilityAdmission::Supported => Ok(()),
        CapabilityAdmission::Missing => {
            anyhow::bail!("selected offering is missing {name} capability evidence")
        }
        CapabilityAdmission::InsufficientEvidence => {
            anyhow::bail!("selected offering has insufficient {name} capability evidence")
        }
    }
}

fn validate_admitted_request(
    admitted: Option<&AdmittedModelCapabilities>,
    tools: &[omegon_traits::ToolDefinition],
    options: &StreamOptions,
) -> anyhow::Result<()> {
    let Some(admitted) = admitted else {
        return Ok(());
    };
    if !tools.is_empty() {
        if !admitted.provider_supports_tools {
            anyhow::bail!("serving provider contribution declares tools unsupported");
        }
        validate_capability(admitted.tools, "tool")?;
    }
    if options.reasoning.is_some() {
        validate_capability(admitted.reasoning, "reasoning")?;
    }
    Ok(())
}

async fn resolve_provider_route(
    model_spec: &str,
    secrets: Option<&omegon_secrets::SecretsManager>,
    exact: bool,
) -> Option<ResolvedProviderRoute> {
    let selected_model = crate::providers::canonical_model_spec(model_spec);
    let selected_provider = crate::providers::infer_provider_id(&selected_model);
    if selected_provider == crate::providers::zen::PROVIDER_ID {
        crate::providers::zen::ensure_available(crate::providers::model_id_from_spec(
            &selected_model,
        ))
        .await
        .ok()?;
    }
    let providers = if exact {
        vec![selected_provider.as_str()]
    } else {
        crate::providers::fallback_order_for_model(&selected_model)
    };
    for provider in providers {
        if let Some(resolution) =
            crate::providers::resolve_provider_binding_with_secrets(provider, secrets).await
        {
            let serving_model = if exact {
                selected_model.clone()
            } else {
                format!(
                    "{}:{}",
                    provider,
                    crate::providers::model_id_from_spec(&selected_model)
                )
            };
            if !exact && provider != selected_provider {
                tracing::info!(requested = %selected_provider, resolved = provider, model_spec, "falling back to declared compatible provider route");
            }
            return Some(ResolvedProviderRoute {
                selected_model,
                serving_model: serving_model.clone(),
                credential_source_class: resolution.credential_source_class,
                native_model: serving_model.clone(),
                endpoint_provenance: None,
                admitted_capabilities: None,
                bridge: resolution.bridge,
            });
        }
    }
    tracing::warn!(requested = %selected_provider, model_spec, "no executable provider route available");
    None
}

struct ProviderLoopRouteSetup {
    controller: Option<std::sync::Arc<crate::route::RouteController>>,
    warmup: Option<crate::ollama::OllamaManager>,
}

#[async_trait::async_trait]
impl crate::loop_driver::LoopRouteSetupContract for ProviderLoopRouteSetup {
    async fn invalidate(&self, model: &str, detail: &str) -> bool {
        match self.controller.as_ref() {
            Some(controller) => controller.invalidate_serving_model(model, detail).await,
            None => false,
        }
    }

    async fn serving_model(&self) -> Option<String> {
        let controller = self.controller.as_ref()?;
        controller
            .snapshot()
            .await
            .serving_model()
            .map(str::to_string)
    }

    async fn prepare(
        &self,
        route: &crate::loop_driver::LoopRoute,
        events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    ) {
        let route = LoopRoute {
            selected_model: route.selected_model.clone(),
            serving_model: route.serving_model.clone(),
            provider_id: route.provider_id.clone(),
            schema_dialect: route.schema_dialect.clone(),
            contribution_generation_id: route.contribution_generation_id.clone(),
            normalizer_contribution_id: route.normalizer_contribution_id.clone(),
            normalizer_generation_id: route.normalizer_generation_id.clone(),
            options: StreamOptions::default(),
        };
        prepare_loop_route(&route, events, self.warmup.as_ref()).await;
    }
}

pub(crate) fn loop_route_setup(
    controller: Option<std::sync::Arc<crate::route::RouteController>>,
    warmup: Option<crate::ollama::OllamaManager>,
) -> crate::loop_driver::LoopRouteSetup {
    crate::loop_driver::LoopRouteSetup::new(std::sync::Arc::new(ProviderLoopRouteSetup {
        controller,
        warmup,
    }))
}

#[derive(Debug, Clone)]
pub(crate) struct LoopRoute {
    pub(crate) selected_model: String,
    pub(crate) serving_model: String,
    pub(crate) provider_id: String,
    pub(crate) schema_dialect: String,
    pub(crate) contribution_generation_id: String,
    pub(crate) normalizer_contribution_id: omegon_traits::RuntimeContributionId,
    pub(crate) normalizer_generation_id: omegon_traits::RuntimeContributionGenerationId,
    pub(crate) options: StreamOptions,
}

#[derive(Clone)]
pub(crate) struct LoopRoutePolicy {
    pub(crate) selected_model: String,
    pub(crate) bridge_model: Option<String>,
    pub(crate) extended_context: bool,
    pub(crate) settings: Option<crate::settings::SharedSettings>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopRouteFailure {
    ContextOverflow,
    MalformedHistory,
    Exhausted,
    Other,
}

pub(crate) struct LoopRouteRequest<'a> {
    pub(crate) route: &'a LoopRoute,
    pub(crate) system_prompt: &'a str,
    pub(crate) messages: &'a [LlmMessage],
    pub(crate) tools: &'a [omegon_traits::ToolDefinition],
    pub(crate) events: &'a tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    pub(crate) max_retries: u32,
    pub(crate) retry_delay_ms: u64,
    pub(crate) cancel_keeps_prompt: Option<&'a std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) scope: &'a crate::invocation_service::InvocationScope,
    pub(crate) step_id: Uuid,
    pub(crate) semantic_request: Option<&'a crate::loop_driver::LoopModelRequestIdentity>,
    pub(crate) response_facts: Option<&'a dyn crate::loop_driver::LoopResponseFactContract>,
}

pub(crate) struct LoopRouteDispatch {
    pub(crate) message: AssistantMessage,
    pub(crate) durable_route: Option<DurableRouteIdentity>,
    pub(crate) response_attempt_ordinal: u32,
}

pub(crate) struct LoopCompactionRequest<'a> {
    pub(crate) payload: &'a str,
    pub(crate) options: &'a StreamOptions,
    pub(crate) selected_model: &'a str,
    pub(crate) scope: &'a crate::invocation_service::InvocationScope,
    pub(crate) step_id: Uuid,
    pub(crate) authority: Option<&'a dyn crate::loop_driver::LoopCompactionAuthority>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderStopNotice {
    pub(crate) provider: String,
    pub(crate) reason: String,
    pub(crate) message: String,
}

pub(crate) async fn loop_startup_route(
    bridge: &dyn LlmBridge,
    setup: &crate::loop_driver::LoopRouteSetup,
    policy: &LoopRoutePolicy,
) -> LoopRoute {
    let route_model = if let Some(serving_model) = setup.serving_model().await {
        serving_model
    } else {
        policy
            .bridge_model
            .as_ref()
            .unwrap_or(&policy.selected_model)
            .clone()
    };
    let serving_model = bridge
        .native_model_hint()
        .unwrap_or(&route_model)
        .to_string();
    let selected_model = bridge
        .selected_model_hint()
        .unwrap_or(&policy.selected_model)
        .to_string();
    let endpoint_provenance = bridge.endpoint_route_provenance_hint();
    let provider_id = endpoint_provenance.map_or_else(
        || crate::providers::infer_provider_id(&serving_model),
        |provenance| provenance.selected_provider_id.clone(),
    );
    let schema_dialect = if let Some(provenance) = endpoint_provenance {
        provenance.schema_dialect.clone()
    } else {
        crate::provider_contributions::registry()
            .get(&provider_id)
            .expect("serving provider contribution must exist")
            .tools
            .dialect_name()
            .into()
    };
    let contribution_generation_id = endpoint_provenance.map_or_else(
        || {
            crate::provider_contributions::registry()
                .get(&provider_id)
                .expect("serving provider contribution must exist")
                .owner_generation_id
                .as_str()
                .to_string()
        },
        |provenance| provenance.contribution_generation_id.clone(),
    );
    let (normalizer_contribution_id, normalizer_generation_id) = tool_schema_normalizer_identity();
    LoopRoute {
        provider_id,
        schema_dialect,
        contribution_generation_id,
        normalizer_contribution_id,
        normalizer_generation_id,
        selected_model,
        options: StreamOptions {
            model: Some(serving_model.clone()),
            reasoning: None,
            extended_context: policy.extended_context,
            ..Default::default()
        },
        serving_model,
    }
}

pub(crate) async fn loop_turn_route(
    bridge: &dyn LlmBridge,
    setup: &crate::loop_driver::LoopRouteSetup,
    policy: &LoopRoutePolicy,
    base: &StreamOptions,
) -> LoopRoute {
    let mut route = loop_startup_route(bridge, setup, policy).await;
    route.options = base.clone();
    route.options.reasoning = policy.settings.as_ref().and_then(|settings| {
        let guard = settings.lock().ok()?;
        match guard.thinking {
            crate::settings::ThinkingLevel::Off => None,
            level => Some(level.as_str().to_string()),
        }
    });
    if bridge.endpoint_route_provenance_hint().is_some() {
        route.options.model = Some(route.serving_model.clone());
        return route;
    }
    route.serving_model = if let Some(serving_model) = setup.serving_model().await {
        serving_model
    } else {
        policy.bridge_model.clone().unwrap_or_else(|| {
            policy
                .settings
                .as_ref()
                .and_then(|settings| settings.lock().ok().map(|guard| guard.model.clone()))
                .unwrap_or_else(|| policy.selected_model.clone())
        })
    };
    route.provider_id = crate::providers::infer_provider_id(&route.serving_model);
    let contribution = crate::provider_contributions::registry()
        .get(&route.provider_id)
        .expect("serving provider contribution must exist");
    route.schema_dialect = contribution.tools.dialect_name().into();
    (
        route.normalizer_contribution_id,
        route.normalizer_generation_id,
    ) = tool_schema_normalizer_identity();
    route.options.model = Some(route.serving_model.clone());
    route
}

pub(crate) async fn prepare_loop_route(
    route: &LoopRoute,
    events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    manager: Option<&crate::ollama::OllamaManager>,
) {
    if route.provider_id != "ollama" {
        return;
    }
    let model_name = crate::providers::model_id_from_spec(&route.serving_model);
    let owned;
    let manager = match manager {
        Some(manager) => manager,
        None => {
            owned = crate::ollama::OllamaManager::new();
            &owned
        }
    };
    if !manager.is_reachable().await {
        tracing::debug!("Ollama not reachable — skipping warmup");
        return;
    }
    let _ = events.send(omegon_traits::AgentEvent::SystemNotification {
        message: format!("⟳ Loading {model_name} into memory…"),
    });
    match manager.warmup_model(model_name).await {
        Ok(crate::ollama::WarmupResult::AlreadyWarm) => {
            tracing::debug!(model_name, "Ollama model already warm");
        }
        Ok(crate::ollama::WarmupResult::WasLoaded) => {
            tracing::info!(model_name, "Ollama model warmed up successfully");
            let _ = events.send(omegon_traits::AgentEvent::SystemNotification {
                message: format!("↯ {model_name} loaded"),
            });
        }
        Err(error) => {
            tracing::warn!(model_name, %error, "Ollama warmup failed — proceeding anyway")
        }
    }
}

pub(crate) fn classify_loop_route_failure(error: &anyhow::Error) -> LoopRouteFailure {
    let message = error.to_string();
    if is_context_overflow(&message) {
        LoopRouteFailure::ContextOverflow
    } else if is_malformed_history(&message) {
        LoopRouteFailure::MalformedHistory
    } else if is_upstream_exhausted(error) {
        LoopRouteFailure::Exhausted
    } else {
        LoopRouteFailure::Other
    }
}

pub(crate) fn is_upstream_exhausted(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .to_lowercase()
        .contains("upstream exhausted:")
}

pub(crate) async fn compact_loop_route(
    bridge: &dyn LlmBridge,
    request: LoopCompactionRequest<'_>,
) -> anyhow::Result<String> {
    bridge.validate_request_capabilities(&[], request.options)?;
    const MAX_COMPACTION_CHARS: usize = 100_000;
    let (_, _, system) = crate::session_compaction::summary_prompt()?;
    let authority_payload = request
        .authority
        .map(|authority| authority.provider_payload(request.payload))
        .unwrap_or(request.payload);
    let authority_input_too_large =
        request.authority.is_some() && authority_payload.len() > MAX_COMPACTION_CHARS;
    let payload = if authority_payload.len() > MAX_COMPACTION_CHARS && request.authority.is_none() {
        tracing::warn!(
            original = authority_payload.len(),
            truncated = MAX_COMPACTION_CHARS,
            "compaction payload truncated to fit provider limits"
        );
        &authority_payload[..authority_payload.floor_char_boundary(MAX_COMPACTION_CHARS)]
    } else {
        authority_payload
    };
    let messages = [LlmMessage::User {
        content: payload.to_string(),
        images: vec![],
    }];
    let requested_model = request
        .options
        .model
        .as_deref()
        .unwrap_or(request.selected_model);
    let serving_model = bridge
        .native_model_hint()
        .map_or_else(|| requested_model.to_string(), str::to_string);
    if let Some(authority) = request.authority {
        let lease = route_lease(
            request.selected_model,
            &serving_model,
            bridge.credential_source_class_hint(),
            authority.compaction_request_id(),
            bridge.endpoint_route_provenance_hint(),
        )?;
        let lease_id = if authority.is_idle() {
            None
        } else {
            let (Some(session_authority), Some(turn_id)) =
                (request.scope.authority.as_ref(), request.scope.turn_id)
            else {
                anyhow::bail!("turn compaction requires complete session authority");
            };
            RouteLeaseOwner::Session {
                authority: session_authority,
                turn_id,
            }
            .record(&lease)?;
            Some(lease.lease_id)
        };
        authority.prepare(crate::loop_driver::LoopCompactionRouteEvidence {
            lease_id,
            selected_provider_id: lease.selected_provider_id,
            selected_model_id: lease.selected_model_id,
            serving_provider_id: lease.serving_provider_id,
            serving_model_id: lease.serving_model_id,
            schema_dialect: lease.schema_dialect,
            credential_source_class: lease.credential_source_class,
            fallback_reason: lease.fallback_reason,
            contribution_generation_id: lease.contribution_generation_id,
            route_policy: lease.route_policy,
            endpoint_id: lease.endpoint_id,
            adapter_id: lease.adapter_id,
            inventory_generation: lease.inventory_generation,
        })?;
        if authority_input_too_large {
            authority.fail(
                crate::session_authority::CompactionRequestOutcome::ProviderFailed,
                "compaction_input_too_large",
            )?;
            anyhow::bail!("Compaction input exceeds provider safety limit");
        }
    } else {
        record_loop_route_lease_for_request(
            request.scope,
            request.step_id,
            request.selected_model,
            &serving_model,
            bridge.credential_source_class_hint(),
            None,
            bridge.endpoint_route_provenance_hint(),
        )?;
    }
    let mut rx = match bridge
        .stream(&system, &messages, &[], request.options)
        .await
    {
        Ok(receiver) => receiver,
        Err(error) => {
            if let Some(authority) = request.authority {
                authority.fail(
                    crate::session_authority::CompactionRequestOutcome::ProviderFailed,
                    "provider_dispatch_failed",
                )?;
            }
            return Err(error);
        }
    };
    let mut summary = String::new();
    let mut done = false;
    let mut timed_out = false;
    while let Some(event) = match tokio::time::timeout(Duration::from_secs(120), rx.recv()).await {
        Ok(event) => event,
        Err(_) => {
            tracing::warn!("summary stream idle timeout");
            timed_out = true;
            None
        }
    } {
        match event {
            LlmEvent::TextDelta { delta } => summary.push_str(&delta),
            LlmEvent::Done { .. } => {
                done = true;
                break;
            }
            LlmEvent::Error { message } => {
                if let Some(authority) = request.authority {
                    authority.fail(
                        crate::session_authority::CompactionRequestOutcome::ProviderFailed,
                        "provider_error",
                    )?;
                }
                anyhow::bail!("Compaction LLM error: {message}")
            }
            _ => {}
        }
    }
    if !done {
        if let Some(authority) = request.authority {
            authority.fail(
                if timed_out {
                    crate::session_authority::CompactionRequestOutcome::TimedOut
                } else {
                    crate::session_authority::CompactionRequestOutcome::Eof
                },
                if timed_out {
                    "stream_timed_out"
                } else {
                    "provider_eof"
                },
            )?;
        }
        anyhow::bail!(if timed_out {
            "Compaction timed out before provider Done"
        } else {
            "Compaction reached EOF before provider Done"
        });
    }
    if summary.is_empty() {
        if let Some(authority) = request.authority {
            authority.fail(
                crate::session_authority::CompactionRequestOutcome::ProviderFailed,
                "provider_empty_response",
            )?;
        }
        anyhow::bail!("Compaction produced empty summary");
    }
    if let Some(authority) = request.authority {
        authority.commit_done(&summary)?;
    }
    tracing::info!(summary_len = summary.len(), "Compaction summary received");
    Ok(summary)
}

pub(crate) async fn dispatch_loop_route(
    bridge: &dyn LlmBridge,
    request: LoopRouteRequest<'_>,
) -> anyhow::Result<LoopRouteDispatch> {
    bridge.validate_request_capabilities(request.tools, &request.route.options)?;
    let serving_model = bridge
        .native_model_hint()
        .unwrap_or(&request.route.serving_model)
        .to_string();
    let durable_route = record_loop_route_lease_for_request(
        request.scope,
        request.step_id,
        &request.route.selected_model,
        &serving_model,
        bridge.credential_source_class_hint(),
        request.semantic_request,
        bridge.endpoint_route_provenance_hint(),
    )?;

    let mut attempt = 0u32;
    let message_id = Uuid::new_v4();
    let mut delay = request.retry_delay_ms;
    let started = Instant::now();
    loop {
        attempt += 1;
        let error = match bridge
            .stream(
                request.system_prompt,
                request.messages,
                request.tools,
                &request.route.options,
            )
            .await
        {
            Ok(mut receiver) => match consume_llm_stream_with_policy(
                &mut receiver,
                request.events,
                &request.route.provider_id,
                &request.route.serving_model,
                request.cancel_keeps_prompt,
                StreamIdlePolicy::from_env(),
                request.response_facts,
                request.semantic_request,
                message_id,
                attempt - 1,
            )
            .await
            {
                Ok(message) => {
                    return Ok(LoopRouteDispatch {
                        message,
                        durable_route,
                        response_attempt_ordinal: attempt - 1,
                    });
                }
                Err(error) => error,
            },
            Err(error) => error,
        };
        if error.is::<crate::bridge::ProviderRouteUnavailable>() {
            close_failed_response_request(&request, attempt - 1)?;
            return Err(error);
        }
        let server_retry_delay_ms = error
            .downcast_ref::<crate::upstream_errors::UpstreamResponseFailure>()
            .and_then(|failure| failure.retry_after_ms);
        let error_message = error.to_string();
        if error_message.starts_with("durable response terminated at transport EOF") {
            return Err(error);
        }
        let class =
            classify_upstream_error_for_provider(&request.route.provider_id, &error_message);
        let transient_kind = class.transient_kind();
        if transient_kind.is_none() {
            if attempt > 1 {
                tracing::error!(class = class.label(), recovery = ?class.recovery_action(), "LLM error after {attempt} attempts: {error_message}");
            }
            if !is_context_overflow(&error_message) && !is_malformed_history(&error_message) {
                close_failed_response_request(&request, attempt - 1)?;
            }
            return Err(error);
        }

        let kind_label = class.label();
        if request.semantic_request.is_some() && request.response_facts.is_none() {
            anyhow::bail!(
                "durable response retry requires matching request and authority fact contracts"
            );
        }

        let elapsed = started.elapsed();
        let persistent_overload = persistent_interactive_overload_retry(
            request.max_retries,
            &request.route.provider_id,
            transient_kind,
        );
        let rate_limit_exhausted = request.max_retries == 0
            && matches!(transient_kind, Some(TransientFailureKind::RateLimited))
            && elapsed.as_secs() >= 120;
        let stall_exhausted = request.max_retries == 0
            && matches!(transient_kind, Some(TransientFailureKind::StalledStream))
            && elapsed.as_secs()
                >= stall_exhaustion_secs(
                    &request.route.provider_id,
                    &request.route.serving_model,
                    request.route.options.reasoning.as_deref(),
                );
        let envelope_exhausted = !persistent_overload
            && transient_retry_envelope_exhausted(
                request.max_retries,
                transient_kind,
                elapsed.as_secs(),
            );
        let attempt_exhausted = request.max_retries > 0 && attempt >= request.max_retries;
        if attempt_exhausted || rate_limit_exhausted || stall_exhausted || envelope_exhausted {
            if request.semantic_request.is_none() {
                let _ = append_upstream_failure_log(&UpstreamFailureLogEntry {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    provider: request.route.provider_id.clone(),
                    model: request.route.serving_model.clone(),
                    failure_kind: kind_label.to_string(),
                    internal_class: kind_label.to_string(),
                    recovery_action: class.recovery_action(),
                    attempt,
                    request_id: None,
                    response_attempt_ordinal: None,
                    delay_ms: 0,
                    message: error_message.clone(),
                });
            }
            let reason = if rate_limit_exhausted {
                "session rate-limit exhaustion"
            } else if stall_exhausted {
                "stream stall exhaustion"
            } else if envelope_exhausted {
                "transient retry exhaustion"
            } else {
                "upstream exhausted"
            };
            let advice = exhaustion_advice(
                &request.route.provider_id,
                transient_kind,
                rate_limit_exhausted,
                stall_exhausted,
            );
            let _ = request
                .events
                .send(omegon_traits::AgentEvent::ProviderFailure {
                    provider: request.route.provider_id.clone(),
                    model: request.route.serving_model.clone(),
                    reason: kind_label.to_string(),
                    attempts: attempt,
                    message: error_message.clone(),
                    retryable: false,
                    recommended_action: advice.to_string(),
                });
            let _ = request.events.send(omegon_traits::AgentEvent::SystemNotification {
                message: format!(
                    "🛑 {} {reason}: {attempt} consecutive {kind_label} failures over {:.0}s. {advice}",
                    request.route.provider_id,
                    elapsed.as_secs_f64()
                ),
            });
            if !matches!(
                transient_kind,
                Some(
                    TransientFailureKind::Timeout
                        | TransientFailureKind::StalledStream
                        | TransientFailureKind::ResponseCancelled
                )
            ) {
                close_failed_response_request(&request, attempt - 1)?;
            }
            anyhow::bail!(
                "{reason}: {} consecutive {} failures over {:.0}s: {}",
                attempt,
                kind_label,
                elapsed.as_secs_f64(),
                error_message
            );
        }

        if let (Some(facts), Some(identity)) = (request.response_facts, request.semantic_request) {
            facts.fail_attempt(
                identity,
                attempt - 1,
                response_attempt_failure(transient_kind.expect("transient kind checked")),
                response_attempt_failure_reason(transient_kind.expect("transient kind checked")),
            )?;
        }

        let base_delay = delay;
        let retry_delay = jittered_retry_delay_ms(
            base_delay,
            attempt,
            &request.route.provider_id,
            &request.route.serving_model,
        );
        let retry_delay = select_retry_delay_ms(
            retry_delay,
            server_retry_delay_ms,
            remaining_retry_envelope_ms(
                request.max_retries,
                persistent_overload,
                transient_kind,
                elapsed,
                &request.route.provider_id,
                &request.route.serving_model,
                request.route.options.reasoning.as_deref(),
            ),
        );
        if request.semantic_request.is_none() {
            let _ = append_upstream_failure_log(&UpstreamFailureLogEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                provider: request.route.provider_id.clone(),
                model: request.route.serving_model.clone(),
                failure_kind: kind_label.to_string(),
                internal_class: kind_label.to_string(),
                recovery_action: class.recovery_action(),
                attempt,
                request_id: None,
                response_attempt_ordinal: None,
                delay_ms: retry_delay,
                message: error_message.clone(),
            });
        }
        if matches!(attempt, 10 | 25 | 50 | 100) || (attempt > 100 && attempt.is_multiple_of(100)) {
            let _ = request.events.send(omegon_traits::AgentEvent::SystemNotification {
                message: format!(
                    "⚠ {} is seeing repeated transient upstream failures: {attempt} consecutive {kind_label} failures over {:.0}s — credentials still look valid; switch only if this persists",
                    request.route.provider_id,
                    elapsed.as_secs_f64()
                ),
            });
        }
        let operator_detail = transient_kind
            .map(|kind| kind.operator_detail(&request.route.provider_id, &error_message))
            .unwrap_or_else(|| crate::util::truncate_str(&error_message, 300).to_string());
        let _ = request
            .events
            .send(omegon_traits::AgentEvent::ProviderRetry {
                provider: request.route.provider_id.clone(),
                model: request.route.serving_model.clone(),
                attempt,
                delay_ms: retry_delay,
                reason: kind_label.to_string(),
                message: operator_detail.clone(),
                recoverable: true,
            });
        let _ = request.events.send(omegon_traits::AgentEvent::SystemNotification {
            message: format!(
                "⚠ Upstream {kind_label} — retrying (attempt {attempt}, delay {retry_delay}ms): {operator_detail}"
            ),
        });
        tokio::time::sleep(Duration::from_millis(retry_delay)).await;
        delay = base_delay.saturating_mul(2).min(15_000);
    }
}

fn response_attempt_failure(
    kind: TransientFailureKind,
) -> crate::loop_driver::LoopResponseAttemptFailure {
    match kind {
        TransientFailureKind::Timeout | TransientFailureKind::StalledStream => {
            crate::loop_driver::LoopResponseAttemptFailure::TimedOut
        }
        TransientFailureKind::ResponseIncomplete => {
            crate::loop_driver::LoopResponseAttemptFailure::Eof
        }
        TransientFailureKind::NetworkConnect
        | TransientFailureKind::NetworkReset
        | TransientFailureKind::Dns
        | TransientFailureKind::DecodeBody
        | TransientFailureKind::BridgeDropped
        | TransientFailureKind::ResponseCancelled => {
            crate::loop_driver::LoopResponseAttemptFailure::TransportLost
        }
        TransientFailureKind::RateLimited
        | TransientFailureKind::ProviderOverloaded
        | TransientFailureKind::Upstream5xx => {
            crate::loop_driver::LoopResponseAttemptFailure::ProviderError
        }
    }
}

fn response_attempt_failure_reason(kind: TransientFailureKind) -> &'static str {
    match kind {
        TransientFailureKind::RateLimited => "rate_limited",
        TransientFailureKind::ProviderOverloaded => "provider_overloaded",
        TransientFailureKind::Upstream5xx => "upstream_5xx",
        TransientFailureKind::Timeout => "timeout",
        TransientFailureKind::StalledStream => "stalled_stream",
        TransientFailureKind::NetworkConnect => "connection_failure",
        TransientFailureKind::NetworkReset => "connection_reset",
        TransientFailureKind::Dns => "dns_failure",
        TransientFailureKind::DecodeBody => "unreadable_response_body",
        TransientFailureKind::BridgeDropped => "bridge_dropped_stream",
        TransientFailureKind::ResponseIncomplete => "response_truncated",
        TransientFailureKind::ResponseCancelled => "response_cancelled",
    }
}

fn close_failed_response_request(
    request: &LoopRouteRequest<'_>,
    response_attempt_ordinal: u32,
) -> anyhow::Result<()> {
    if let (Some(facts), Some(identity)) = (request.response_facts, request.semantic_request) {
        facts.close_request(
            identity,
            response_attempt_ordinal,
            crate::loop_driver::LoopRequestTerminal::ProviderFailed,
            "provider_failed",
        )?;
    }
    Ok(())
}

fn persistent_interactive_overload_retry(
    max_retries: u32,
    provider: &str,
    transient_kind: Option<TransientFailureKind>,
) -> bool {
    max_retries == 0
        && provider == "openai-codex"
        && matches!(
            transient_kind,
            Some(TransientFailureKind::ProviderOverloaded)
        )
}

fn jittered_retry_delay_ms(base_delay_ms: u64, attempt: u32, provider: &str, model: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    provider.hash(&mut hasher);
    model.hash(&mut hasher);
    attempt.hash(&mut hasher);
    let half = base_delay_ms / 2;
    half.saturating_add(hasher.finish() % base_delay_ms.saturating_sub(half).max(1))
}

fn select_retry_delay_ms(
    fallback_delay_ms: u64,
    server_delay_ms: Option<u64>,
    remaining_envelope_ms: Option<u64>,
) -> u64 {
    let selected = server_delay_ms.unwrap_or(fallback_delay_ms);
    remaining_envelope_ms.map_or(selected, |remaining| selected.min(remaining.max(1)))
}

#[allow(clippy::too_many_arguments)]
fn remaining_retry_envelope_ms(
    max_retries: u32,
    persistent_overload: bool,
    transient_kind: Option<TransientFailureKind>,
    elapsed: Duration,
    provider: &str,
    model: &str,
    reasoning: Option<&str>,
) -> Option<u64> {
    if max_retries > 0 || persistent_overload {
        return None;
    }
    let envelope = match transient_kind {
        Some(TransientFailureKind::RateLimited) => Duration::from_secs(120),
        Some(TransientFailureKind::StalledStream) => {
            Duration::from_secs(stall_exhaustion_secs(provider, model, reasoning))
        }
        Some(_) => Duration::from_secs(600),
        None => return None,
    };
    Some(u64::try_from(envelope.saturating_sub(elapsed).as_millis()).unwrap_or(u64::MAX))
}

fn transient_retry_envelope_exhausted(
    max_retries: u32,
    transient_kind: Option<TransientFailureKind>,
    elapsed_secs: u64,
) -> bool {
    max_retries == 0
        && !matches!(
            transient_kind,
            Some(TransientFailureKind::RateLimited | TransientFailureKind::StalledStream)
        )
        && elapsed_secs >= 600
}

fn stall_exhaustion_secs(provider: &str, model: &str, reasoning: Option<&str>) -> u64 {
    let long_reasoning = provider == "openai-codex"
        || ((provider == "openai" || provider == "openai-compatible")
            && (model.contains("gpt-5")
                || crate::providers::model_id_from_spec(model) == "gpt-6-astra"
                || model.contains("o3")
                || model.contains("o4")));
    if long_reasoning {
        return match reasoning {
            Some("high" | "xhigh" | "max") => 2_400,
            Some("medium") => 1_800,
            Some("low" | "minimal") | None | Some(_) => 1_200,
        };
    }
    600
}

fn exhaustion_advice(
    provider: &str,
    transient_kind: Option<TransientFailureKind>,
    rate_limit_exhausted: bool,
    stall_exhausted: bool,
) -> &'static str {
    if stall_exhausted {
        if provider == "anthropic"
            && crate::providers::anthropic_credential_mode()
                == crate::providers::AnthropicCredentialMode::OAuthOnly
        {
            return "Anthropic OAuth streams are repeatedly stalling. Retry /auth login anthropic to refresh the Claude session, or switch provider with /model.";
        }
        if matches!(provider, "openai-codex" | "openai" | "openai-compatible") {
            return "The OpenAI stream exceeded Omegon's local silent-reasoning budget. This may be a long-running reasoning window or a wedged stream; lower thinking, retry later, or switch provider with /model.";
        }
        return "The provider's stream is unresponsive. Retry later or switch provider with /model.";
    }
    if rate_limit_exhausted || matches!(transient_kind, Some(TransientFailureKind::RateLimited)) {
        return "This provider is rate-limiting the session. Wait for reset or switch provider with /model.";
    }
    match transient_kind {
        Some(TransientFailureKind::ProviderOverloaded | TransientFailureKind::Upstream5xx) => {
            "This is a provider-side outage or capacity problem. Retry later, switch provider with /model, or check the provider status page."
        }
        Some(
            TransientFailureKind::Timeout
            | TransientFailureKind::NetworkConnect
            | TransientFailureKind::NetworkReset
            | TransientFailureKind::Dns
            | TransientFailureKind::DecodeBody
            | TransientFailureKind::BridgeDropped
            | TransientFailureKind::ResponseIncomplete
            | TransientFailureKind::ResponseCancelled,
        ) => {
            "The provider or network path is unstable. Retry later or switch provider with /model."
        }
        Some(TransientFailureKind::StalledStream) => {
            "The provider's stream is unresponsive. Retry later or switch provider with /model."
        }
        Some(TransientFailureKind::RateLimited) | None => {
            "Retry later or switch provider with /model."
        }
    }
}

pub(crate) fn provider_stop_notice(
    route: &LoopRoute,
    raw: &serde_json::Value,
) -> Option<ProviderStopNotice> {
    let reason = raw
        .get("provider_stop_reason")
        .and_then(serde_json::Value::as_str)
        .filter(|reason| !reason.trim().is_empty())?;
    let abnormal = match route.provider_id.as_str() {
        "openai" | "openrouter" | "openai-compatible" => {
            !matches!(reason, "stop" | "tool_calls" | "function_call")
        }
        "anthropic" => !matches!(reason, "end_turn" | "tool_use" | "stop_sequence"),
        _ => matches!(
            reason,
            "length" | "max_tokens" | "content_filter" | "safety" | "incomplete"
        ),
    };
    if !abnormal {
        return None;
    }
    let hint = match reason {
        "length" | "max_tokens" => {
            "The provider stopped because the output limit was reached; the visible answer may be incomplete."
        }
        "content_filter" | "safety" => {
            "The provider stopped because safety/content filtering intervened; the visible answer may be incomplete."
        }
        _ => "The provider ended the response abnormally; the visible answer may be incomplete.",
    };
    Some(ProviderStopNotice {
        provider: route.provider_id.clone(),
        reason: reason.to_string(),
        message: format!(
            "Provider stop: {}/{reason}\n{hint}\nUse a continuation prompt or retry with a larger output budget if needed.",
            route.provider_id
        ),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamIdleState {
    AwaitingFirstEvent = 0,
    OutputStreaming = 1,
    ToolStreaming = 2,
    ReasoningStreaming = 3,
    AmbiguousSilent = 4,
}

impl StreamIdleState {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::OutputStreaming,
            2 => Self::ToolStreaming,
            3 => Self::ReasoningStreaming,
            4 => Self::AmbiguousSilent,
            _ => Self::AwaitingFirstEvent,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::AwaitingFirstEvent => "awaiting first stream event",
            Self::OutputStreaming => "output streaming",
            Self::ToolStreaming => "tool-call streaming",
            Self::ReasoningStreaming => "reasoning streaming",
            Self::AmbiguousSilent => "ambiguous silent reasoning",
        }
    }

    fn is_ambiguous_reasoning(self) -> bool {
        matches!(self, Self::ReasoningStreaming | Self::AmbiguousSilent)
    }
}

#[derive(Debug, Clone, Copy)]
struct StreamIdlePolicy {
    initial: Duration,
    active: Duration,
    reasoning: Duration,
    absolute: Duration,
}

impl StreamIdlePolicy {
    fn from_env() -> Self {
        let initial = duration_from_env("OMEGON_LLM_INITIAL_IDLE_TIMEOUT_SECS", 30, 90);
        let reasoning = duration_from_env("OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS", 60, 600);
        let absolute = duration_from_env("OMEGON_LLM_ABSOLUTE_TIMEOUT_SECS", 60, 1800);
        Self {
            initial,
            active: Duration::from_secs(90),
            reasoning,
            absolute,
        }
    }

    fn budget(self, phase: StreamIdleState, visible_output_seen: bool) -> Duration {
        if visible_output_seen {
            return self.active;
        }
        match phase {
            StreamIdleState::AwaitingFirstEvent
            | StreamIdleState::ReasoningStreaming
            | StreamIdleState::AmbiguousSilent => self.reasoning,
            StreamIdleState::OutputStreaming | StreamIdleState::ToolStreaming => self.active,
        }
    }
}

fn duration_from_env(name: &str, minimum: u64, default: u64) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds >= minimum)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(default))
}

fn stream_idle_phase_after_event(current: StreamIdleState, event: &LlmEvent) -> StreamIdleState {
    use crate::bridge::BoundaryExpectation;
    match event {
        LlmEvent::Start | LlmEvent::TransportHeartbeat => current,
        LlmEvent::TextStart | LlmEvent::TextDelta { .. } => StreamIdleState::OutputStreaming,
        LlmEvent::TextEnd | LlmEvent::ThinkingEnd | LlmEvent::ToolCallEnd { .. } => {
            StreamIdleState::AmbiguousSilent
        }
        LlmEvent::ThinkingStart | LlmEvent::ThinkingDelta { .. } => {
            StreamIdleState::ReasoningStreaming
        }
        LlmEvent::ToolCallStart | LlmEvent::ToolCallDelta { .. } => StreamIdleState::ToolStreaming,
        LlmEvent::Boundary { expectation } => match expectation {
            BoundaryExpectation::MoreReasoning => StreamIdleState::ReasoningStreaming,
            BoundaryExpectation::MoreContent => StreamIdleState::OutputStreaming,
            BoundaryExpectation::Unknown => StreamIdleState::AmbiguousSilent,
            BoundaryExpectation::Terminal => current,
        },
        LlmEvent::ProviderContinuity { .. }
        | LlmEvent::Done { .. }
        | LlmEvent::Error { .. }
        | LlmEvent::UpstreamFailure { .. } => current,
    }
}

const MAX_DURABLE_RESPONSE_CHUNK_BYTES: usize = 64 * 1024;
const DURABLE_RESPONSE_FLUSH_INTERVAL: Duration = Duration::from_millis(50);

struct DurableResponseProjection<'a> {
    facts: Option<&'a dyn crate::loop_driver::LoopResponseFactContract>,
    request: Option<&'a crate::loop_driver::LoopModelRequestIdentity>,
    message_id: Uuid,
    response_attempt_ordinal: u32,
    pending_kind: Option<crate::loop_driver::LoopResponseContentKind>,
    pending: String,
    pending_since: Option<tokio::time::Instant>,
    text_ordinal: u32,
    thinking_ordinal: u32,
    receipts: Vec<crate::loop_driver::LoopResponseChunkReceipt>,
}

impl<'a> DurableResponseProjection<'a> {
    fn new(
        facts: Option<&'a dyn crate::loop_driver::LoopResponseFactContract>,
        request: Option<&'a crate::loop_driver::LoopModelRequestIdentity>,
        message_id: Uuid,
        response_attempt_ordinal: u32,
    ) -> anyhow::Result<Self> {
        if facts.is_some() && request.is_none() {
            anyhow::bail!(
                "durable response emission requires matching request and session contracts"
            );
        }
        Ok(Self {
            facts,
            request,
            message_id,
            response_attempt_ordinal,
            pending_kind: None,
            pending: String::new(),
            pending_since: None,
            text_ordinal: 0,
            thinking_ordinal: 0,
            receipts: Vec::new(),
        })
    }

    fn enabled(&self) -> bool {
        self.facts.is_some()
    }

    fn flush_deadline(&self) -> Option<tokio::time::Instant> {
        self.pending_since
            .map(|started| started + DURABLE_RESPONSE_FLUSH_INTERVAL)
    }

    fn push(
        &mut self,
        kind: crate::loop_driver::LoopResponseContentKind,
        delta: &str,
        events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    ) -> anyhow::Result<()> {
        if !self.enabled() {
            self.broadcast(kind, delta, events);
            return Ok(());
        }
        if delta.is_empty() {
            return Ok(());
        }
        if self.pending_kind.is_some_and(|pending| pending != kind) {
            self.flush(events)?;
        }
        if self.pending.is_empty() {
            self.pending_kind = Some(kind);
            self.pending_since = Some(tokio::time::Instant::now());
        }
        self.pending.push_str(delta);
        while self.pending.len() >= MAX_DURABLE_RESPONSE_CHUNK_BYTES {
            let split = self
                .pending
                .floor_char_boundary(MAX_DURABLE_RESPONSE_CHUNK_BYTES);
            let remainder = self.pending.split_off(split);
            self.flush(events)?;
            self.pending = remainder;
            if !self.pending.is_empty() {
                self.pending_kind = Some(kind);
                self.pending_since = Some(tokio::time::Instant::now());
            }
        }
        Ok(())
    }

    fn flush(
        &mut self,
        events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    ) -> anyhow::Result<()> {
        if self.pending.is_empty() {
            self.pending_kind = None;
            self.pending_since = None;
            return Ok(());
        }
        let kind = self
            .pending_kind
            .expect("non-empty response buffer has a kind");
        let bytes = std::mem::take(&mut self.pending);
        let ordinal = match kind {
            crate::loop_driver::LoopResponseContentKind::Text => &mut self.text_ordinal,
            crate::loop_driver::LoopResponseContentKind::Thinking => &mut self.thinking_ordinal,
        };
        let receipt = self
            .facts
            .expect("enabled projection has response facts")
            .append_content(
                self.request
                    .expect("enabled projection has request identity"),
                self.message_id,
                self.response_attempt_ordinal,
                kind,
                *ordinal,
                bytes.as_bytes(),
            )?;
        *ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("assistant chunk ordinal overflow"))?;
        self.receipts.push(receipt);
        self.pending_kind = None;
        self.pending_since = None;
        self.broadcast(kind, &bytes, events);
        Ok(())
    }

    fn broadcast(
        &self,
        kind: crate::loop_driver::LoopResponseContentKind,
        text: &str,
        events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    ) {
        let event = match kind {
            crate::loop_driver::LoopResponseContentKind::Text => {
                omegon_traits::AgentEvent::MessageChunk { text: text.into() }
            }
            crate::loop_driver::LoopResponseContentKind::Thinking => {
                omegon_traits::AgentEvent::ThinkingChunk { text: text.into() }
            }
        };
        let _ = events.send(event);
    }

    fn store_continuity(
        &self,
        route_provider: &str,
        route_model: &str,
        kind: crate::bridge::ProviderContinuityKind,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let Some(facts) = self.facts else {
            return Ok(());
        };
        let contribution = crate::provider_contributions::registry()
            .get(route_provider)
            .ok_or_else(|| anyhow::anyhow!("serving provider contribution is absent"))?;
        let crate::provider_contributions::ProviderContinuityPolicy::RestrictedRequired {
            allowed_kinds,
            max_blob_bytes,
        } = &contribution.continuity
        else {
            return Ok(());
        };
        if !allowed_kinds.contains(&kind)
            || bytes.is_empty()
            || bytes.len() as u64 > *max_blob_bytes
        {
            anyhow::bail!("provider continuity violates the captured contribution policy");
        }
        let map_kind = |kind| match kind {
            crate::bridge::ProviderContinuityKind::HiddenReasoning => {
                crate::loop_driver::LoopProviderContinuityKind::HiddenReasoning
            }
            crate::bridge::ProviderContinuityKind::OpaqueProviderState => {
                crate::loop_driver::LoopProviderContinuityKind::OpaqueProviderState
            }
        };
        let allowed = allowed_kinds
            .iter()
            .copied()
            .map(map_kind)
            .collect::<Vec<_>>();
        facts.store_continuity(
            self.request
                .expect("enabled projection has request identity"),
            self.response_attempt_ordinal,
            route_provider,
            crate::providers::model_id_from_spec(route_model),
            contribution.owner_generation_id.as_str(),
            map_kind(kind),
            &allowed,
            *max_blob_bytes,
            bytes,
        )
    }

    fn commit(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        tool_call_count: usize,
    ) -> anyhow::Result<()> {
        let Some(facts) = self.facts else {
            return Ok(());
        };
        let request = self
            .request
            .expect("enabled projection has request identity");
        facts.commit_message(
            request,
            self.message_id,
            self.response_attempt_ordinal,
            &self.receipts,
            Some((input_tokens, output_tokens)),
            u32::try_from(tool_call_count)?,
        )
    }
}

#[allow(clippy::too_many_arguments)]
async fn consume_llm_stream_with_policy(
    receiver: &mut tokio::sync::mpsc::Receiver<LlmEvent>,
    events: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    provider: &str,
    model: &str,
    cancel_keeps_prompt: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
    idle_policy: StreamIdlePolicy,
    response_facts: Option<&dyn crate::loop_driver::LoopResponseFactContract>,
    semantic_request: Option<&crate::loop_driver::LoopModelRequestIdentity>,
    message_id: Uuid,
    response_attempt_ordinal: u32,
) -> anyhow::Result<AssistantMessage> {
    let mut text_parts: Vec<String> = Vec::new();
    let mut thinking_parts: Vec<String> = Vec::new();
    let mut tool_calls = Vec::new();
    let mut final_raw = serde_json::Value::Null;
    let mut provider_tokens = (0, 0, 0, 0);
    let mut provider_telemetry = None;
    let mut completed = false;
    let mut visible_output_seen = false;
    let mut stream_started = false;
    let started_at = tokio::time::Instant::now();
    let absolute_deadline = started_at + idle_policy.absolute;
    let mut last_semantic_progress = started_at;
    let mut transport_heartbeats = 0u64;
    let mut recent_text_len = 0usize;
    let mut repetition_window = Vec::new();
    const REPETITION_WINDOW_SIZE: usize = 40;
    const REPETITION_ABORT_THRESHOLD: usize = 30;
    let stream_idle_phase =
        std::sync::atomic::AtomicU8::new(StreamIdleState::AwaitingFirstEvent as u8);
    let mut durable = DurableResponseProjection::new(
        response_facts,
        semantic_request,
        message_id,
        response_attempt_ordinal,
    )?;

    let _ = events.send(omegon_traits::AgentEvent::MessageStart {
        role: "assistant".into(),
    });
    'stream: loop {
        let event = match tokio::time::timeout(
            durable
                .flush_deadline()
                .map(|deadline| deadline.saturating_duration_since(tokio::time::Instant::now()))
                .unwrap_or(Duration::MAX)
                .min(
                    idle_policy
                        .budget(
                            StreamIdleState::from_u8(
                                stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed),
                            ),
                            visible_output_seen,
                        )
                        .saturating_sub(last_semantic_progress.elapsed())
                        .min(
                            absolute_deadline
                                .saturating_duration_since(tokio::time::Instant::now()),
                        ),
                ),
            receiver.recv(),
        )
        .await
        {
            Ok(event) => event,
            Err(_) => {
                if durable
                    .flush_deadline()
                    .is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
                {
                    durable.flush(events)?;
                    continue 'stream;
                }
                let phase = StreamIdleState::from_u8(
                    stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed),
                );
                let idle = idle_policy.budget(phase, visible_output_seen);
                let reason = if tokio::time::Instant::now() >= absolute_deadline {
                    format!(
                        "LLM stream exceeded the absolute {}s turn deadline during {} — transport received {transport_heartbeats} heartbeat event(s)",
                        idle_policy.absolute.as_secs(),
                        phase.label()
                    )
                } else {
                    format!(
                        "LLM stream made no semantic progress for {}s during {} — transport received {transport_heartbeats} heartbeat event(s)",
                        idle.as_secs(),
                        phase.label()
                    )
                };
                let _ = events.send(omegon_traits::AgentEvent::StreamIdle {
                    provider: provider.to_string(),
                    model: model.to_string(),
                    phase: phase.label().to_string(),
                    idle_secs: idle.as_secs(),
                    ambiguous: phase.is_ambiguous_reasoning() && !visible_output_seen,
                    message: reason.clone(),
                });
                let _ = events.send(omegon_traits::AgentEvent::MessageAbort {
                    reason: Some(reason.clone()),
                });
                anyhow::bail!(reason);
            }
        };
        let Some(event) = event else {
            break;
        };
        let next_phase = stream_idle_phase_after_event(
            StreamIdleState::from_u8(stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed)),
            &event,
        );
        stream_idle_phase.store(next_phase as u8, std::sync::atomic::Ordering::Relaxed);
        match event {
            LlmEvent::Start => {
                if !stream_started {
                    stream_started = true;
                    last_semantic_progress = tokio::time::Instant::now();
                }
            }
            LlmEvent::TransportHeartbeat => {
                transport_heartbeats = transport_heartbeats.saturating_add(1);
            }
            LlmEvent::TextStart | LlmEvent::ThinkingStart | LlmEvent::ToolCallStart => {
                last_semantic_progress = tokio::time::Instant::now();
            }
            LlmEvent::TextDelta { delta } => {
                if !delta.is_empty() {
                    last_semantic_progress = tokio::time::Instant::now();
                    visible_output_seen = true;
                    mark_prompt_replayable(cancel_keeps_prompt);
                }
                durable.push(
                    crate::loop_driver::LoopResponseContentKind::Text,
                    &delta,
                    events,
                )?;
                recent_text_len += delta.len();
                let trimmed = delta.trim().to_lowercase();
                if !trimmed.is_empty() {
                    repetition_window.push(trimmed);
                    if repetition_window.len() > REPETITION_WINDOW_SIZE {
                        repetition_window.remove(0);
                    }
                    if repetition_window.len() >= REPETITION_WINDOW_SIZE {
                        let latest = repetition_window.last().expect("window is non-empty");
                        let matches = repetition_window
                            .iter()
                            .filter(|item| item == &latest)
                            .count();
                        if matches >= REPETITION_ABORT_THRESHOLD {
                            let reason = format!(
                                "Model output degenerate: phrase {latest:?} repeated {matches}/{REPETITION_WINDOW_SIZE} recent chunks — aborting to prevent runaway"
                            );
                            tracing::warn!(repeated_phrase = %latest, matches, total_text_bytes = recent_text_len, "Degenerate repetition detected — aborting stream");
                            let _ = events.send(omegon_traits::AgentEvent::MessageAbort {
                                reason: Some(reason.clone()),
                            });
                            anyhow::bail!(reason);
                        }
                    }
                }
                if let Some(last) = text_parts.last_mut() {
                    last.push_str(&delta);
                } else {
                    text_parts.push(delta);
                }
            }
            LlmEvent::TextEnd => {
                durable.flush(events)?;
                text_parts.push(String::new());
            }
            LlmEvent::ThinkingDelta { delta } => {
                if !delta.is_empty() {
                    last_semantic_progress = tokio::time::Instant::now();
                    mark_prompt_replayable(cancel_keeps_prompt);
                }
                durable.push(
                    crate::loop_driver::LoopResponseContentKind::Thinking,
                    &delta,
                    events,
                )?;
                if let Some(last) = thinking_parts.last_mut() {
                    last.push_str(&delta);
                } else {
                    thinking_parts.push(delta);
                }
            }
            LlmEvent::ThinkingEnd => {
                durable.flush(events)?;
                thinking_parts.push(String::new());
            }
            LlmEvent::ToolCallDelta { delta } => {
                if !delta.is_empty() {
                    last_semantic_progress = tokio::time::Instant::now();
                }
            }
            LlmEvent::ToolCallEnd { tool_call } => {
                durable.flush(events)?;
                last_semantic_progress = tokio::time::Instant::now();
                visible_output_seen = true;
                mark_prompt_replayable(cancel_keeps_prompt);
                tool_calls.push(ToolCall {
                    id: tool_call.id,
                    name: tool_call.name,
                    arguments: tool_call.arguments,
                });
            }
            LlmEvent::Boundary { expectation } => {
                durable.flush(events)?;
                if !matches!(expectation, crate::bridge::BoundaryExpectation::Unknown) {
                    last_semantic_progress = tokio::time::Instant::now();
                }
            }
            LlmEvent::ProviderContinuity { kind, bytes } => {
                durable.flush(events)?;
                durable.store_continuity(provider, model, kind, &bytes)?;
            }
            LlmEvent::Done {
                message,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                provider_telemetry: telemetry,
            } => {
                final_raw = message.get("raw").cloned().unwrap_or(message);
                provider_tokens = (
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    cache_creation_tokens,
                );
                provider_telemetry = telemetry;
                completed = true;
                durable.flush(events)?;
                durable.commit(input_tokens, output_tokens, tool_calls.len())?;
                break;
            }
            LlmEvent::Error { message } => {
                durable.flush(events)?;
                let _ = events.send(omegon_traits::AgentEvent::MessageAbort {
                    reason: Some(message.clone()),
                });
                anyhow::bail!("LLM error: {message}");
            }
            LlmEvent::UpstreamFailure { failure } => {
                durable.flush(events)?;
                let _ = events.send(omegon_traits::AgentEvent::MessageAbort {
                    reason: Some(failure.message.clone()),
                });
                return Err(failure.into());
            }
        }
    }
    durable.flush(events)?;
    let _ = events.send(omegon_traits::AgentEvent::MessageEnd);
    if !completed {
        if let (Some(facts), Some(request)) = (response_facts, semantic_request) {
            facts.close_request(
                request,
                response_attempt_ordinal,
                crate::loop_driver::LoopRequestTerminal::Eof,
                "transport_eof",
            )?;
            anyhow::bail!("durable response terminated at transport EOF without provider Done");
        }
        anyhow::bail!("LLM stream ended without a completion event — the bridge may have crashed");
    }
    while text_parts.last().is_some_and(String::is_empty) {
        text_parts.pop();
    }
    while thinking_parts.last().is_some_and(String::is_empty) {
        thinking_parts.pop();
    }
    Ok(AssistantMessage {
        text: text_parts.join(""),
        thinking: (!thinking_parts.is_empty()).then(|| thinking_parts.join("")),
        tool_calls,
        raw: final_raw,
        provider_tokens,
        provider_telemetry,
    })
}

fn mark_prompt_replayable(
    cancel_keeps_prompt: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
) {
    if let Some(cancel_keeps_prompt) = cancel_keeps_prompt {
        cancel_keeps_prompt.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn route_lease(
    selected_model: &str,
    serving_model: &str,
    credential_source_class: Option<&str>,
    request_id: Option<Uuid>,
    endpoint_provenance: Option<&crate::bridge::EndpointRouteProvenance>,
) -> anyhow::Result<ProviderRouteLease> {
    if let Some(provenance) = endpoint_provenance {
        let selected_provider_id = provenance.selected_provider_id.clone();
        let selected_model_id = selected_model
            .strip_prefix(&format!("{selected_provider_id}:"))
            .unwrap_or(selected_model)
            .to_string();
        let serving_model_id = serving_model
            .strip_prefix(&format!("{selected_provider_id}:"))
            .unwrap_or(serving_model)
            .to_string();
        return Ok(ProviderRouteLease {
            schema_version: ROUTE_LEASE_SCHEMA_VERSION,
            lease_id: Uuid::new_v4(),
            request_id: request_id.unwrap_or_else(Uuid::new_v4),
            selected_provider_id: selected_provider_id.clone(),
            selected_model_id,
            serving_provider_id: selected_provider_id,
            serving_model_id,
            schema_dialect: provenance.schema_dialect.clone(),
            credential_source_class: credential_source_class
                .unwrap_or("bearer_token")
                .to_string(),
            fallback_reason: None,
            contribution_generation_id: provenance.contribution_generation_id.clone(),
            route_policy: "admitted_manifest_endpoint_v1".into(),
            endpoint_id: Some(provenance.endpoint_id.clone()),
            adapter_id: Some(provenance.adapter_id.clone()),
            inventory_generation: Some(provenance.inventory_generation),
        });
    }
    let selected_provider_id = crate::providers::infer_provider_id(selected_model);
    let serving_provider_id = crate::providers::infer_provider_id(serving_model);
    let contribution = crate::provider_contributions::registry()
        .get(&serving_provider_id)
        .ok_or_else(|| anyhow::anyhow!("serving provider contribution is absent"))?;
    let fallback = selected_provider_id != serving_provider_id;

    Ok(ProviderRouteLease {
        schema_version: ROUTE_LEASE_SCHEMA_VERSION,
        lease_id: Uuid::new_v4(),
        request_id: request_id.unwrap_or_else(Uuid::new_v4),
        selected_provider_id,
        selected_model_id: crate::providers::model_id_from_spec(selected_model).to_string(),
        serving_provider_id,
        serving_model_id: crate::providers::model_id_from_spec(serving_model).to_string(),
        schema_dialect: contribution.tools.dialect_name().to_string(),
        credential_source_class: credential_source_class
            .unwrap_or_else(|| contribution.authentication.as_str())
            .to_string(),
        fallback_reason: fallback.then(|| "selected_provider_unavailable".to_string()),
        contribution_generation_id: contribution.owner_generation_id.as_str().to_string(),
        route_policy: if fallback {
            "declared_model_family_fallback_v1"
        } else {
            "selected_provider_only_v1"
        }
        .to_string(),
        endpoint_id: None,
        adapter_id: None,
        inventory_generation: None,
    })
}

fn recorded_at_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn native_admission_fable_family_accepts_ordinary_tool_requests() {
        use crate::inference_inventory::{EvidenceKind, InventoryLayer, InventorySnapshot};
        let inventory = InventorySnapshot::build(
            1,
            vec![InventoryLayer::embedded_registry(
                crate::model_registry::ModelRegistry::global(),
            )],
        )
        .unwrap();
        let tools = [omegon_traits::ToolDefinition {
            name: "read".into(),
            label: "Read".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type":"object"}),
            capabilities: Vec::new(),
        }];
        for model in ["anthropic:claude-fable-5-1", "anthropic:claude-mythos-5-1"] {
            let offering = admit_exact_route(&inventory, model, &["tools".into()])
                .expect("Fable5.1 and Mythos5.1 support ordinary tools");
            let capabilities = AdmittedModelCapabilities {
                tools: offering_capability_admission(offering, "tools", EvidenceKind::Declared),
                reasoning: offering_capability_admission(
                    offering,
                    "reasoning",
                    EvidenceKind::Declared,
                ),
                provider_supports_tools: true,
            };
            validate_admitted_request(Some(&capabilities), &tools, &StreamOptions::default())
                .expect("admitted Fable-family tools must reach the native request boundary");
        }
    }

    #[tokio::test]
    #[allow(
        clippy::await_holding_lock,
        reason = "serialize synthetic credential fixtures through native bridge resolution"
    )]
    async fn native_admission_credentials_construct_bridges_after_discovery() {
        use crate::inference_inventory::{InventoryLayer, InventorySnapshot};
        use base64::Engine as _;
        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    unsafe {
                        match value {
                            Some(value) => std::env::set_var(key, value),
                            None => std::env::remove_var(key),
                        }
                    }
                }
            }
        }
        let _guard = crate::auth::TEST_AUTH_ENV_LOCK.lock().unwrap();
        let fixture = tempfile::tempdir().unwrap();
        let _restore = Restore(
            [
                "OMEGON_AUTH_JSON_PATH",
                "OPENAI_API_KEY",
                "ANTHROPIC_API_KEY",
            ]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
        );
        unsafe {
            std::env::set_var("OMEGON_AUTH_JSON_PATH", fixture.path().join("auth.json"));
            std::env::set_var("OPENAI_API_KEY", "native-openai-fixture");
            std::env::set_var("ANTHROPIC_API_KEY", "native-anthropic-fixture");
        }
        let claims = serde_json::json!({"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account"}});
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
        crate::auth::write_credentials(
            "openai-codex",
            &crate::auth::OAuthCredentials {
                cred_type: "oauth".into(),
                access: format!("eyJhbGciOiJub25lIn0.{payload}.fixture"),
                refresh: "unused-fixture-refresh".into(),
                expires: u64::MAX,
            },
        )
        .unwrap();
        let embedded =
            InventoryLayer::embedded_registry(crate::model_registry::ModelRegistry::global());
        for (provider, model) in [
            ("openai-codex", "gpt-6-astra"),
            ("anthropic", "claude-fable-5-1"),
            ("openai", "gpt-6-astra"),
        ] {
            let selected = format!("{provider}:{model}");
            assert!(crate::route::CredentialLedger.probe(provider).is_valid());
            let discovered = crate::inference_discovery::DiscoveredModels {
                endpoint_id: provider.into(),
                models: vec![crate::inference_discovery::DiscoveredModel {
                    id: model.into(),
                    ..Default::default()
                }],
                fetched_at: 1,
                ttl_secs: 60,
                cached: false,
            };
            let known =
                BTreeMap::from([(provider.to_string(), BTreeSet::from([model.to_string()]))]);
            let discovery =
                crate::inference_discovery::build_discovery_layer(&[discovered], &known);
            for layers in [
                vec![embedded.clone()],
                vec![embedded.clone(), discovery.clone()],
            ] {
                let inventory = InventorySnapshot::build(1, layers).unwrap();
                let resolved = ProviderRouteService
                    .resolve_exact_admitted(&selected, None, &inventory, &[])
                    .await
                    .unwrap_or_else(|| {
                        panic!(
                            "valid {provider} credentials must construct the admitted native bridge"
                        )
                    });
                assert_eq!(resolved.serving_model, selected);
                assert!(resolved.endpoint_provenance.is_none());
                if provider == "openai-codex" {
                    assert_eq!(resolved.credential_source_class, "stored_oauth");
                }
                let controller = crate::route::RouteController::default();
                let snapshot = controller
                    .switch_model(
                        selected.clone(),
                        &crate::route::CredentialLedger,
                        Some(resolved.into_unleased_bridge()),
                    )
                    .await
                    .unwrap();
                assert_eq!(snapshot.serving_model(), Some(selected.as_str()));
            }
            use crate::inference_inventory::{
                AdapterId, EndpointId, EndpointPatch, EvidenceKind, InventorySource, OfferingId,
                OfferingPatch, TransportSpec,
            };
            for field in [
                "transport",
                "managed_transport",
                "adapter",
                "secret_refs",
                "native_model_id",
                "endpoint",
            ] {
                let mut layer =
                    InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
                match field {
                    "transport" | "managed_transport" => {
                        layer.endpoints.insert(
                            EndpointId(provider.into()),
                            EndpointPatch {
                                transport: Some(if field == "transport" {
                                    TransportSpec::Http {
                                        base_url: "https://example.invalid/v1".into(),
                                    }
                                } else {
                                    TransportSpec::Managed
                                }),
                                ..Default::default()
                            },
                        );
                    }
                    "adapter" => {
                        layer.endpoints.insert(
                            EndpointId(provider.into()),
                            EndpointPatch {
                                adapter: Some(AdapterId(AdapterId::CHAT_COMPLETIONS.into())),
                                ..Default::default()
                            },
                        );
                    }
                    "secret_refs" => {
                        layer.endpoints.insert(
                            EndpointId(provider.into()),
                            EndpointPatch {
                                secret_refs: Some(vec!["OPENAI_API_KEY".into()]),
                                ..Default::default()
                            },
                        );
                    }
                    "native_model_id" => {
                        layer.offerings.insert(
                            OfferingId(selected.clone()),
                            OfferingPatch {
                                native_model_id: Some(model.into()),
                                ..Default::default()
                            },
                        );
                    }
                    "endpoint" => {
                        layer.offerings.insert(
                            OfferingId(selected.clone()),
                            OfferingPatch {
                                endpoint: Some(EndpointId(provider.into())),
                                ..Default::default()
                            },
                        );
                    }
                    _ => unreachable!(),
                }
                for layers in [
                    vec![embedded.clone(), layer.clone()],
                    vec![embedded.clone(), layer, discovery.clone()],
                ] {
                    let inventory = InventorySnapshot::build(2, layers);
                    if field == "managed_transport" {
                        assert!(
                            inventory.is_err(),
                            "custom Managed transports must be rejected before route construction"
                        );
                        continue;
                    }
                    let inventory = inventory.unwrap();
                    assert!(
                        ProviderRouteService
                            .resolve_exact_admitted(&selected, None, &inventory, &[])
                            .await
                            .is_none(),
                        "{provider} {field} override must retain manifest validation even after discovery"
                    );
                }
            }
            let mut remapped =
                InventoryLayer::new(InventorySource::Discovery, EvidenceKind::Discovered);
            remapped.offerings.insert(
                OfferingId(selected.clone()),
                OfferingPatch {
                    native_model_id: Some("different-model".into()),
                    ..Default::default()
                },
            );
            let inventory = InventorySnapshot::build(2, vec![embedded.clone(), remapped]).unwrap();
            let retained = ProviderRouteService
                .resolve_exact_admitted(&selected, None, &inventory, &[])
                .await
                .expect("declared identity survives discovery");
            assert_eq!(
                retained.native_model, selected,
                "discovery cannot silently remap native model identity"
            );
        }
    }

    #[tokio::test]
    #[allow(
        clippy::await_holding_lock,
        reason = "serialize process-global credential fixtures through native resolution and restoration"
    )]
    async fn astra_embedded_openai_uses_native_factory_and_override_stays_manifest() {
        use crate::inference_inventory::{
            EndpointId, EndpointPatch, EvidenceKind, InventoryLayer, InventorySnapshot,
            InventorySource, TransportSpec,
        };
        struct RestoreEnv(Option<std::ffi::OsString>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                unsafe {
                    match self.0.take() {
                        Some(value) => std::env::set_var("OPENAI_API_KEY", value),
                        None => std::env::remove_var("OPENAI_API_KEY"),
                    }
                }
            }
        }
        let _lock = crate::auth::TEST_AUTH_ENV_LOCK.lock().unwrap();
        let _restore = RestoreEnv(std::env::var_os("OPENAI_API_KEY"));
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "fixture-api-key");
        }
        let embedded =
            InventoryLayer::embedded_registry(crate::model_registry::ModelRegistry::global());
        let inventory = InventorySnapshot::build(1, vec![embedded.clone()]).unwrap();
        let route = ProviderRouteService
            .resolve_exact_admitted("openai:gpt-6-astra", None, &inventory, &["tools".into()])
            .await
            .expect("embedded native API route should resolve using canonical OPENAI_API_KEY");
        assert_eq!(route.selected_model, "openai:gpt-6-astra");
        assert!(
            route.endpoint_provenance.is_none(),
            "native client must not become a manifest ChatCompletions adapter"
        );
        let mut override_layer =
            InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
        override_layer.endpoints.insert(
            EndpointId("openai".into()),
            EndpointPatch {
                transport: Some(TransportSpec::Http {
                    base_url: "https://example.invalid/v1".into(),
                }),
                ..Default::default()
            },
        );
        for field in ["adapter", "secret_refs", "native_model_id", "endpoint"] {
            let mut field_override =
                InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
            match field {
                "adapter" => {
                    field_override.endpoints.insert(
                        EndpointId("openai".into()),
                        EndpointPatch {
                            adapter: Some(crate::inference_inventory::AdapterId(
                                "chat-completions".into(),
                            )),
                            ..Default::default()
                        },
                    );
                }
                "secret_refs" => {
                    field_override.endpoints.insert(
                        EndpointId("openai".into()),
                        EndpointPatch {
                            secret_refs: Some(vec!["OPENAI_API_KEY".into()]),
                            ..Default::default()
                        },
                    );
                }
                "native_model_id" => {
                    field_override.offerings.insert(
                        crate::inference_inventory::OfferingId("openai:gpt-6-astra".into()),
                        crate::inference_inventory::OfferingPatch {
                            native_model_id: Some("gpt-6-astra".into()),
                            ..Default::default()
                        },
                    );
                }
                "endpoint" => {
                    field_override.offerings.insert(
                        crate::inference_inventory::OfferingId("openai:gpt-6-astra".into()),
                        crate::inference_inventory::OfferingPatch {
                            endpoint: Some(EndpointId("openai".into())),
                            ..Default::default()
                        },
                    );
                }
                _ => unreachable!(),
            }
            let snapshot =
                InventorySnapshot::build(2, vec![embedded.clone(), field_override]).unwrap();
            assert!(
                ProviderRouteService
                    .resolve_exact_admitted(
                        "openai:gpt-6-astra",
                        None,
                        &snapshot,
                        &["tools".into()]
                    )
                    .await
                    .is_none(),
                "{field} override must retain manifest validation"
            );
        }
        let overridden = InventorySnapshot::build(2, vec![embedded, override_layer]).unwrap();
        assert!(
            ProviderRouteService
                .resolve_exact_admitted("openai:gpt-6-astra", None, &overridden, &["tools".into()])
                .await
                .is_none(),
            "project endpoint override must retain manifest secret/provenance validation"
        );
    }

    #[test]
    fn exact_route_admission_rejects_model_absent_from_current_inventory() {
        let snapshot = crate::inference_inventory::InventorySnapshot::empty();
        let rejection = admit_exact_route(&snapshot, "anthropic:claude-fable-5", &[])
            .expect_err("absent exact offering must fail closed");
        assert!(matches!(
            rejection,
            crate::inference_inventory::ExactAdmissionRejection::UnknownOffering(_)
        ));
    }

    fn manifest_endpoint_snapshot(adapter: &str) -> crate::inference_inventory::InventorySnapshot {
        manifest_endpoint_snapshot_at(adapter, "https://inference.internal/v1")
    }

    fn manifest_endpoint_snapshot_at(
        adapter: &str,
        base_url: &str,
    ) -> crate::inference_inventory::InventorySnapshot {
        manifest_endpoint_snapshot_with_secret(
            adapter,
            base_url,
            "OMEGON_PROJECT_ENDPOINT_707269766174652D656E64706F696E74_TOKEN",
        )
    }

    fn manifest_endpoint_snapshot_with_secret(
        adapter: &str,
        base_url: &str,
        secret_ref: &str,
    ) -> crate::inference_inventory::InventorySnapshot {
        use crate::inference_inventory::{
            AdapterId, EndpointId, EndpointPatch, EvidenceKind, InventoryLayer, InventorySource,
            Modality, OfferingId, OfferingPatch, TransportSpec,
        };
        let mut layer = InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
        layer.endpoints.insert(
            EndpointId("private-endpoint".into()),
            EndpointPatch {
                adapter: Some(AdapterId(adapter.into())),
                transport: Some(TransportSpec::Http {
                    base_url: base_url.into(),
                }),
                secret_refs: Some(vec![secret_ref.into()]),
                enabled: Some(true),
                ..Default::default()
            },
        );
        layer.offerings.insert(
            OfferingId("private-chat:stable-chat".into()),
            OfferingPatch {
                endpoint: Some(EndpointId("private-endpoint".into())),
                native_model_id: Some("model-v3".into()),
                display_name: Some("Stable chat".into()),
                input_modalities: Some(BTreeSet::from([Modality(Modality::TEXT.into())])),
                output_modalities: Some(BTreeSet::from([Modality(Modality::TEXT.into())])),
                capabilities: BTreeMap::from([("tools".into(), true), ("reasoning".into(), true)]),
                enabled: Some(true),
                ..Default::default()
            },
        );
        crate::inference_inventory::InventorySnapshot::build(42, vec![layer]).unwrap()
    }

    #[test]
    fn standalone_manifest_offering_is_exactly_admitted_with_native_alias() {
        let snapshot =
            manifest_endpoint_snapshot(crate::inference_inventory::AdapterId::CHAT_COMPLETIONS);

        let offering = admit_exact_route(&snapshot, "private-chat:stable-chat", &[]).unwrap();

        assert_eq!(offering.id.0, "private-chat:stable-chat");
        assert_eq!(offering.native_model_id.value, "model-v3");
    }

    #[test]
    fn manifest_endpoint_plan_retains_transport_alias_and_generation() {
        let snapshot =
            manifest_endpoint_snapshot(crate::inference_inventory::AdapterId::CHAT_COMPLETIONS);

        let plan =
            admit_manifest_endpoint_route(&snapshot, "private-chat:stable-chat", &[]).unwrap();

        assert_eq!(plan.selected_offering_id, "private-chat:stable-chat");
        assert_eq!(plan.selected_provider_id, "private-chat");
        assert_eq!(plan.selected_model_id, "stable-chat");
        assert_eq!(plan.native_model_id, "model-v3");
        assert_eq!(plan.endpoint_id, "private-endpoint");
        assert_eq!(plan.inventory_generation, 42);
        assert_eq!(plan.adapter_id, "chat-completions");
        assert_eq!(plan.base_url, "https://inference.internal/v1");
        assert_eq!(
            plan.secret_ref,
            "OMEGON_PROJECT_ENDPOINT_707269766174652D656E64706F696E74_TOKEN"
        );
    }

    #[test]
    fn unsupported_manifest_adapter_fails_closed_during_plan_admission() {
        let snapshot = manifest_endpoint_snapshot("unsupported-wire-v7");

        let error = admit_manifest_endpoint_route(&snapshot, "private-chat:stable-chat", &[])
            .expect_err("unsupported adapter must not produce an execution plan");

        assert!(error.to_string().contains("not executable"));
    }

    #[test]
    fn manifest_endpoint_rejects_unbound_secret_and_remote_plaintext_http() {
        assert_ne!(
            manifest_endpoint_secret_name(
                crate::inference_inventory::InventorySource::Project,
                "corp-prod",
            ),
            manifest_endpoint_secret_name(
                crate::inference_inventory::InventorySource::Project,
                "corp_prod",
            )
        );
        assert_ne!(
            manifest_endpoint_secret_name(
                crate::inference_inventory::InventorySource::Project,
                "corp-prod",
            ),
            manifest_endpoint_secret_name(
                crate::inference_inventory::InventorySource::User,
                "corp-prod",
            )
        );
        let unbound = manifest_endpoint_snapshot_with_secret(
            crate::inference_inventory::AdapterId::CHAT_COMPLETIONS,
            "https://inference.internal/v1",
            "ANTHROPIC_API_KEY",
        );
        let error = admit_manifest_endpoint_route(&unbound, "private-chat:stable-chat", &[])
            .expect_err("project endpoint must not claim an unrelated credential");
        assert!(
            error
                .to_string()
                .contains("OMEGON_PROJECT_ENDPOINT_707269766174652D656E64706F696E74_TOKEN")
        );

        let plaintext = manifest_endpoint_snapshot_at(
            crate::inference_inventory::AdapterId::CHAT_COMPLETIONS,
            "http://inference.internal/v1",
        );
        let error = admit_manifest_endpoint_route(&plaintext, "private-chat:stable-chat", &[])
            .expect_err("remote bearer transport must use TLS");
        assert!(error.to_string().contains("HTTPS or a loopback host"));
    }

    #[tokio::test]
    async fn unsupported_manifest_adapter_fails_before_secret_resolution() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("secret-recipe-ran");
        let secrets = omegon_secrets::SecretsManager::new(directory.path()).unwrap();
        secrets
            .set_recipe(
                "OMEGON_PROJECT_ENDPOINT_707269766174652D656E64706F696E74_TOKEN",
                &format!("cmd:touch {}", marker.display()),
            )
            .unwrap();
        let snapshot = manifest_endpoint_snapshot("unsupported-wire-v7");

        let route = ProviderRouteService
            .resolve_exact_admitted("private-chat:stable-chat", Some(&secrets), &snapshot, &[])
            .await;

        assert!(route.is_none());
        assert!(!marker.exists(), "unsupported adapter resolved its secret");
    }

    #[tokio::test]
    async fn manifest_endpoint_dispatch_uses_native_model_and_records_provenance() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let expected_len = loop {
                let mut chunk = [0u8; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0, "request closed before headers completed");
                request.extend_from_slice(&chunk[..count]);
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap();
                    break header_end + 4 + content_length;
                }
            };
            while request.len() < expected_len {
                let mut chunk = [0u8; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0, "request closed before body completed");
                request.extend_from_slice(&chunk[..count]);
            }
            let body = b"data: {\"choices\":[{\"delta\":{\"content\":\"manifest reply\"},\"finish_reason\":\"stop\"}]}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.write_all(body).await.unwrap();
            String::from_utf8(request).unwrap()
        });

        let snapshot = manifest_endpoint_snapshot_at(
            crate::inference_inventory::AdapterId::CHAT_COMPLETIONS,
            &base_url,
        );
        let directory = tempfile::tempdir().unwrap();
        let secret_path = directory.path().join("manifest-token");
        std::fs::write(&secret_path, "manifest-secret").unwrap();
        let secrets = omegon_secrets::SecretsManager::new(directory.path()).unwrap();
        secrets
            .set_recipe(
                "OMEGON_PROJECT_ENDPOINT_707269766174652D656E64706F696E74_TOKEN",
                &format!("file:{}", secret_path.display()),
            )
            .unwrap();
        let route = ProviderRouteService
            .resolve_exact_admitted("private-chat:stable-chat", Some(&secrets), &snapshot, &[])
            .await
            .expect("admitted endpoint with a resolvable secret");
        let lease_path = directory.path().join("leases.jsonl");
        let recorder = StepRouteLeaseRecorder::at_path(Uuid::new_v4(), lease_path.clone());
        let lease = route_lease(
            &route.selected_model,
            &route.native_model,
            Some(&route.credential_source_class),
            None,
            route.endpoint_provenance.as_ref(),
        )
        .unwrap();
        recorder.record(&lease).unwrap();
        let bridge = route.into_unleased_bridge();
        assert_eq!(
            bridge.selected_model_hint(),
            Some("private-chat:stable-chat")
        );
        assert_eq!(bridge.native_model_hint(), Some("private-chat:model-v3"));
        let mut events = bridge
            .stream(
                "system",
                &[],
                &[],
                &StreamOptions {
                    model: Some("private-chat:stable-chat".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut reply = None;
        while let Some(event) = events.recv().await {
            if let LlmEvent::Done { message, .. } = event {
                reply = message["text"].as_str().map(str::to_string);
                break;
            }
        }
        assert_eq!(reply.as_deref(), Some("manifest reply"));

        let request = server.await.unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer manifest-secret\r\n")
        );
        let body: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["model"], "model-v3");

        let fact: StepRouteLeaseFact =
            serde_json::from_str(std::fs::read_to_string(lease_path).unwrap().trim()).unwrap();
        assert_eq!(fact.lease.selected_model_id, "stable-chat");
        assert_eq!(fact.lease.serving_model_id, "model-v3");
        assert_eq!(fact.lease.endpoint_id.as_deref(), Some("private-endpoint"));
        assert_eq!(fact.lease.adapter_id.as_deref(), Some("chat-completions"));
        assert_eq!(fact.lease.inventory_generation, Some(42));
    }

    #[test]
    fn session_manifest_provenance_uses_a_new_fact_without_widening_lease_v1() {
        let (directory, authority, scope, request) = staged_request();
        let provenance = crate::bridge::EndpointRouteProvenance {
            selected_provider_id: "private-chat".into(),
            endpoint_id: "private-endpoint".into(),
            adapter_id: "chat-completions".into(),
            inventory_generation: 42,
            contribution_generation_id: MANIFEST_CHAT_COMPLETIONS_GENERATION.into(),
            schema_dialect: "open_ai".into(),
        };
        let lease = route_lease(
            "private-chat:stable-chat",
            "private-chat:model-v3",
            Some("declared_bearer_secret"),
            Some(request.request_id),
            Some(&provenance),
        )
        .unwrap();
        RouteLeaseOwner::Session {
            authority: scope.authority.as_ref().unwrap(),
            turn_id: scope.turn_id.unwrap(),
        }
        .record(&lease)
        .unwrap();

        let state = authority.state();
        assert_eq!(
            state.route_endpoint_provenance[&lease.lease_id].endpoint_id,
            "private-endpoint"
        );
        let facts = std::fs::read_to_string(directory.path().join("session.authority.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let lease_fact = facts
            .iter()
            .find(|fact| fact["event_type"] == "route.lease_recorded")
            .unwrap();
        assert!(lease_fact["payload"].get("endpoint_id").is_none());
        assert!(
            facts
                .iter()
                .any(|fact| fact["event_type"] == "route.endpoint_provenance_recorded")
        );
    }

    #[tokio::test]
    async fn manifest_turn_route_preserves_native_identity_and_host_generation() {
        let snapshot =
            manifest_endpoint_snapshot(crate::inference_inventory::AdapterId::CHAT_COMPLETIONS);
        let plan =
            admit_manifest_endpoint_route(&snapshot, "private-chat:stable-chat", &[]).unwrap();
        let bridge =
            construct_manifest_endpoint_route(plan, "unused-secret".into()).into_unleased_bridge();
        let setup = loop_route_setup(None, None);
        let policy = LoopRoutePolicy {
            selected_model: "private-chat:stable-chat".into(),
            bridge_model: Some("private-chat:stable-chat".into()),
            extended_context: false,
            settings: None,
        };
        let startup = loop_startup_route(bridge.as_ref(), &setup, &policy).await;
        let turn = loop_turn_route(bridge.as_ref(), &setup, &policy, &startup.options).await;

        assert_eq!(turn.selected_model, "private-chat:stable-chat");
        assert_eq!(turn.serving_model, "private-chat:model-v3");
        assert_eq!(turn.provider_id, "private-chat");
        assert_eq!(turn.schema_dialect, "open_ai");
        assert_eq!(
            turn.contribution_generation_id,
            MANIFEST_CHAT_COMPLETIONS_GENERATION
        );
        assert_eq!(turn.options.model.as_deref(), Some("private-chat:model-v3"));
    }

    #[tokio::test]
    async fn astra_turn_route_forwards_selected_frontier_effort() {
        let model = "openai-codex:gpt-6-astra";
        let shared = crate::settings::shared(model);
        let bridge = crate::bridge::MockBridge { events: vec![] };
        let setup = loop_route_setup(None, None);
        let policy = LoopRoutePolicy {
            selected_model: model.into(),
            bridge_model: Some(model.into()),
            extended_context: false,
            settings: Some(shared.clone()),
        };
        for level in [
            crate::settings::ThinkingLevel::XHigh,
            crate::settings::ThinkingLevel::Max,
        ] {
            shared.lock().unwrap().thinking = level;
            let route = loop_turn_route(&bridge, &setup, &policy, &StreamOptions::default()).await;
            assert_eq!(route.options.reasoning.as_deref(), Some(level.as_str()));
            assert_eq!(route.options.model.as_deref(), Some(model));
        }
    }

    #[tokio::test]
    async fn manifest_compaction_uses_native_identity_and_endpoint_evidence() {
        let bridge = ManifestCompactionBridge {
            provenance: crate::bridge::EndpointRouteProvenance {
                selected_provider_id: "private-chat".into(),
                endpoint_id: "private-endpoint".into(),
                adapter_id: "chat-completions".into(),
                inventory_generation: 42,
                contribution_generation_id: MANIFEST_CHAT_COMPLETIONS_GENERATION.into(),
                schema_dialect: "open_ai".into(),
            },
        };
        let authority = CapturingCompactionAuthority::default();
        let scope = crate::invocation_service::InvocationScope::default();
        let options = StreamOptions {
            model: Some("private-chat:stable-chat".into()),
            ..Default::default()
        };

        let summary = compact_loop_route(
            &bridge,
            LoopCompactionRequest {
                payload: "compact this",
                options: &options,
                selected_model: "private-chat:stable-chat",
                scope: &scope,
                step_id: Uuid::new_v4(),
                authority: Some(&authority),
            },
        )
        .await
        .unwrap();

        assert_eq!(summary, "summary");
        let evidence = authority.evidence.lock().unwrap().clone().unwrap();
        assert_eq!(evidence.selected_provider_id, "private-chat");
        assert_eq!(evidence.selected_model_id, "stable-chat");
        assert_eq!(evidence.serving_provider_id, "private-chat");
        assert_eq!(evidence.serving_model_id, "model-v3");
        assert_eq!(
            evidence.contribution_generation_id,
            MANIFEST_CHAT_COMPLETIONS_GENERATION
        );
        assert_eq!(evidence.endpoint_id.as_deref(), Some("private-endpoint"));
        assert_eq!(evidence.adapter_id.as_deref(), Some("chat-completions"));
        assert_eq!(evidence.inventory_generation, Some(42));
    }

    fn loop_route(provider: &str, model: &str) -> LoopRoute {
        let serving_model = format!("{provider}:{model}");
        let contribution = crate::provider_contributions::registry()
            .get(provider)
            .expect("test provider contribution");
        let (normalizer_contribution_id, normalizer_generation_id) =
            tool_schema_normalizer_identity();
        LoopRoute {
            selected_model: serving_model.clone(),
            serving_model: serving_model.clone(),
            provider_id: provider.to_string(),
            schema_dialect: contribution.tools.dialect_name().into(),
            contribution_generation_id: contribution.owner_generation_id.as_str().into(),
            normalizer_contribution_id,
            normalizer_generation_id,
            options: StreamOptions {
                model: Some(serving_model),
                ..Default::default()
            },
        }
    }

    struct CountingBridge(Arc<AtomicUsize>);

    struct ManifestCompactionBridge {
        provenance: crate::bridge::EndpointRouteProvenance,
    }

    #[derive(Default)]
    struct CapturingCompactionAuthority {
        evidence: std::sync::Mutex<Option<crate::loop_driver::LoopCompactionRouteEvidence>>,
    }

    impl crate::loop_driver::LoopCompactionAuthority for CapturingCompactionAuthority {
        fn provider_payload<'a>(&'a self, fallback: &'a str) -> &'a str {
            fallback
        }

        fn compaction_request_id(&self) -> Option<Uuid> {
            Some(Uuid::new_v4())
        }

        fn is_idle(&self) -> bool {
            true
        }

        fn prepare(
            &self,
            evidence: crate::loop_driver::LoopCompactionRouteEvidence,
        ) -> anyhow::Result<()> {
            *self.evidence.lock().unwrap() = Some(evidence);
            Ok(())
        }

        fn commit_done(&self, _summary: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn fail(
            &self,
            _outcome: crate::session_authority::CompactionRequestOutcome,
            _reason: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl LlmBridge for ManifestCompactionBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            tx.try_send(LlmEvent::TextDelta {
                delta: "summary".into(),
            })?;
            tx.try_send(LlmEvent::Done {
                message: serde_json::json!({}),
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: None,
            })?;
            Ok(rx)
        }

        fn selected_model_hint(&self) -> Option<&str> {
            Some("private-chat:stable-chat")
        }

        fn serving_model_hint(&self) -> Option<&str> {
            Some("private-chat:stable-chat")
        }

        fn native_model_hint(&self) -> Option<&str> {
            Some("private-chat:model-v3")
        }

        fn endpoint_route_provenance_hint(
            &self,
        ) -> Option<&crate::bridge::EndpointRouteProvenance> {
            Some(&self.provenance)
        }

        fn credential_source_class_hint(&self) -> Option<&str> {
            Some("declared_bearer_secret")
        }
    }

    #[async_trait::async_trait]
    impl LlmBridge for CountingBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn admitted_tool_deficient_route_rejects_before_network_dispatch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let bridge = RoutedBridge {
            selected_model: "lab:model".into(),
            serving_model: "lab:model".into(),
            credential_source_class: "test".into(),
            native_model: "lab:model".into(),
            endpoint_provenance: None,
            admitted_capabilities: Some(AdmittedModelCapabilities {
                tools: CapabilityAdmission::Missing,
                reasoning: CapabilityAdmission::Supported,
                provider_supports_tools: true,
            }),
            inner: Box::new(CountingBridge(calls.clone())),
        };
        let tools = [omegon_traits::ToolDefinition {
            name: "read".into(),
            label: "Read".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object"}),
            capabilities: Vec::new(),
        }];

        let result = bridge
            .stream("system", &[], &tools, &StreamOptions::default())
            .await;
        let error = match result {
            Ok(_) => panic!("tool-deficient route reached its transport"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("missing tool capability"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn admitted_reasoning_deficient_route_rejects_before_network_dispatch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let bridge = RoutedBridge {
            selected_model: "lab:model".into(),
            serving_model: "lab:model".into(),
            credential_source_class: "test".into(),
            native_model: "lab:model".into(),
            endpoint_provenance: None,
            admitted_capabilities: Some(AdmittedModelCapabilities {
                tools: CapabilityAdmission::Supported,
                reasoning: CapabilityAdmission::Missing,
                provider_supports_tools: true,
            }),
            inner: Box::new(CountingBridge(calls.clone())),
        };
        let options = StreamOptions {
            reasoning: Some("high".into()),
            ..Default::default()
        };

        let result = bridge.stream("system", &[], &[], &options).await;
        let error = match result {
            Ok(_) => panic!("reasoning-deficient route reached its transport"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("missing reasoning capability"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn provider_tool_contract_remains_stronger_than_offering_evidence() {
        let calls = Arc::new(AtomicUsize::new(0));
        let bridge = RoutedBridge {
            selected_model: "lab:model".into(),
            serving_model: "lab:model".into(),
            credential_source_class: "test".into(),
            native_model: "lab:model".into(),
            endpoint_provenance: None,
            admitted_capabilities: Some(AdmittedModelCapabilities {
                tools: CapabilityAdmission::Supported,
                reasoning: CapabilityAdmission::Supported,
                provider_supports_tools: false,
            }),
            inner: Box::new(CountingBridge(calls.clone())),
        };
        let tools = [omegon_traits::ToolDefinition {
            name: "read".into(),
            label: "Read".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object"}),
            capabilities: Vec::new(),
        }];

        let result = bridge
            .stream("system", &[], &tools, &StreamOptions::default())
            .await;

        assert!(
            result
                .expect_err("unsupported provider tool contract reached transport")
                .to_string()
                .contains("provider contribution declares tools unsupported")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    fn staged_request() -> (
        tempfile::TempDir,
        crate::session_authority::SessionAuthorityHandle,
        crate::invocation_service::InvocationScope,
        crate::loop_driver::LoopModelRequestIdentity,
    ) {
        use crate::loop_session::LoopSemanticFactContract;
        let directory = tempfile::tempdir().unwrap();
        let now = "2026-08-21T12:00:00Z";
        let mut authority = crate::session_authority::SessionAuthority::open(
            &directory.path().join("session.json"),
            "route-semantic",
            "workspace",
            "composition:test",
            crate::session_authority::ActorIdentity {
                principal: "operator".into(),
                ingress: "test".into(),
            },
            now,
        )
        .unwrap();
        let prompt_id = Uuid::new_v4();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                now,
                crate::session_authority::PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: crate::session_authority::QueueMode::UntilReady,
                    content: crate::session_authority::PromptContent {
                        text: "route".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        let turn_id = Uuid::new_v4();
        authority
            .start_turn(Uuid::new_v4(), now, turn_id, prompt_id)
            .unwrap();
        let authority = crate::session_authority::SessionAuthorityHandle::new(authority);
        let scope = crate::invocation_service::InvocationScope {
            session_id: Some("route-semantic".into()),
            turn_id: Some(turn_id),
            authority: Some(authority.clone()),
            ..Default::default()
        };
        let route = loop_route("anthropic", "claude-sonnet-4-6");
        let semantic_route = crate::loop_driver::LoopRoute {
            selected_model: route.selected_model.clone(),
            serving_model: route.serving_model.clone(),
            provider_id: route.provider_id.clone(),
            schema_dialect: route.schema_dialect.clone(),
            contribution_generation_id: route.contribution_generation_id.clone(),
            normalizer_contribution_id: route.normalizer_contribution_id.clone(),
            normalizer_generation_id: route.normalizer_generation_id.clone(),
        };
        let mut adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let step = adapter.start_step().unwrap().unwrap();
        let messages = adapter.current_context_messages(&[]).unwrap();
        let request = adapter
            .prepare_model_request(crate::loop_session::LoopModelRequestCapture {
                step: &step,
                purpose: crate::loop_driver::LoopModelRequestPurpose::Initial,
                replaces: None,
                system_prompt: "system",
                messages: &messages,
                tools: &[],
                tool_lineage: &crate::loop_driver::LoopToolSchemaLineage {
                    composition_generation_id: omegon_traits::RuntimeCompositionGenerationId::new(
                        "composition:test",
                    )
                    .unwrap(),
                    tools: Vec::new(),
                },
                route: &semantic_route,
            })
            .unwrap()
            .unwrap();
        (directory, authority, scope, request)
    }

    struct AuthorityInspectingBridge {
        authority: crate::session_authority::SessionAuthorityHandle,
        request_id: Uuid,
        entries: Arc<AtomicUsize>,
    }

    struct RetryAfterPartialBridge {
        attempts: Arc<AtomicUsize>,
    }

    struct ServerDirectedRetryBridge {
        authority: crate::session_authority::SessionAuthorityHandle,
        request_id: Uuid,
        attempts: Arc<AtomicUsize>,
    }

    struct NonRetryableServerDelayBridge {
        attempts: Arc<AtomicUsize>,
    }

    struct FailContentAppendBridge {
        authority: crate::session_authority::SessionAuthorityHandle,
    }

    struct FailAttemptAppendBridge {
        authority: crate::session_authority::SessionAuthorityHandle,
        attempts: Arc<AtomicUsize>,
    }

    struct OneChunkBridge;

    #[async_trait::async_trait]
    impl LlmBridge for OneChunkBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            tx.try_send(LlmEvent::TextDelta {
                delta: "durable first".into(),
            })
            .unwrap();
            tx.try_send(LlmEvent::Done {
                message: serde_json::json!({}),
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: None,
            })
            .unwrap();
            Ok(rx)
        }
    }

    #[async_trait::async_trait]
    impl LlmBridge for FailContentAppendBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            self.authority.make_next_append_fail();
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            tx.try_send(LlmEvent::TextDelta {
                delta: "hidden".into(),
            })
            .unwrap();
            tx.try_send(LlmEvent::Done {
                message: serde_json::json!({}),
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: None,
            })
            .unwrap();
            Ok(rx)
        }
    }

    #[async_trait::async_trait]
    impl LlmBridge for FailAttemptAppendBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            self.authority.make_next_append_fail();
            anyhow::bail!("connection reset by peer")
        }
    }

    #[async_trait::async_trait]
    impl LlmBridge for RetryAfterPartialBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            if attempt == 0 {
                tx.try_send(LlmEvent::TextDelta {
                    delta: "failed partial".into(),
                })
                .unwrap();
                tx.try_send(LlmEvent::Error {
                    message: "connection reset by peer".into(),
                })
                .unwrap();
            } else {
                tx.try_send(LlmEvent::TextDelta {
                    delta: "successful answer".into(),
                })
                .unwrap();
                tx.try_send(LlmEvent::Done {
                    message: serde_json::json!({}),
                    input_tokens: 3,
                    output_tokens: 2,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    provider_telemetry: None,
                })
                .unwrap();
            }
            Ok(rx)
        }
    }

    #[async_trait::async_trait]
    impl LlmBridge for ServerDirectedRetryBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            if attempt == 0 {
                tx.try_send(LlmEvent::UpstreamFailure {
                    failure: crate::upstream_errors::UpstreamResponseFailure {
                        message: "429 too many requests".into(),
                        retry_after_ms: Some(17_000),
                    },
                })
                .unwrap();
            } else {
                assert!(
                    self.authority
                        .state()
                        .response_attempt_failures
                        .get(&self.request_id)
                        .is_some_and(|failures| failures.contains_key(&0)),
                    "failed-attempt evidence must be durable before retry dispatch"
                );
                tx.try_send(LlmEvent::TextDelta {
                    delta: "recovered".into(),
                })
                .unwrap();
                tx.try_send(LlmEvent::Done {
                    message: serde_json::json!({}),
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    provider_telemetry: None,
                })
                .unwrap();
            }
            Ok(rx)
        }
    }

    #[async_trait::async_trait]
    impl LlmBridge for NonRetryableServerDelayBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(LlmEvent::UpstreamFailure {
                failure: crate::upstream_errors::UpstreamResponseFailure {
                    message: "401 invalid authentication".into(),
                    retry_after_ms: Some(17_000),
                },
            })
            .unwrap();
            Ok(rx)
        }
    }

    #[async_trait::async_trait]
    impl LlmBridge for AuthorityInspectingBridge {
        async fn stream(
            &self,
            _system_prompt: &str,
            _messages: &[LlmMessage],
            _tools: &[omegon_traits::ToolDefinition],
            _options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            let state = self.authority.state();
            let join = state
                .request_route_joins
                .get(&self.request_id)
                .expect("join must be durable before bridge entry");
            assert!(state.route_leases.contains_key(&join.lease_id));
            self.entries.fetch_add(1, Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(LlmEvent::Done {
                message: serde_json::json!({}),
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: None,
            })
            .unwrap();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn durable_lease_and_join_precede_bridge_entry() {
        let (_directory, authority, scope, request_id) = staged_request();
        let entries = Arc::new(AtomicUsize::new(0));
        let bridge = AuthorityInspectingBridge {
            authority,
            request_id: request_id.request_id,
            entries: entries.clone(),
        };
        let (events, _) = tokio::sync::broadcast::channel(4);
        let route = loop_route("anthropic", "claude-sonnet-4-6");
        let dispatch = dispatch_loop_route(
            &bridge,
            LoopRouteRequest {
                route: &route,
                system_prompt: "system",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 1,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: request_id.step_id,
                semantic_request: Some(&request_id),
                response_facts: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(entries.load(Ordering::SeqCst), 1);
        assert_eq!(
            dispatch.durable_route.unwrap().request_id,
            request_id.request_id
        );
    }

    #[tokio::test]
    async fn authority_append_failure_prevents_bridge_entry() {
        let (_directory, authority, scope, request_id) = staged_request();
        authority.make_next_append_fail();
        let entries = Arc::new(AtomicUsize::new(0));
        let bridge = CountingBridge(entries.clone());
        let (events, _) = tokio::sync::broadcast::channel(4);
        let route = loop_route("anthropic", "claude-sonnet-4-6");
        let result = dispatch_loop_route(
            &bridge,
            LoopRouteRequest {
                route: &route,
                system_prompt: "system",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 1,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: request_id.step_id,
                semantic_request: Some(&request_id),
                response_facts: None,
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(entries.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_attempt_chunks_remain_canonical_but_only_done_attempt_commits() {
        let (_directory, authority, scope, request) = staged_request();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let attempts = Arc::new(AtomicUsize::new(0));
        let bridge = RetryAfterPartialBridge {
            attempts: attempts.clone(),
        };
        let (events, mut observer) = tokio::sync::broadcast::channel(16);
        let route = loop_route("anthropic", "claude-sonnet-4-6");

        let dispatch = dispatch_loop_route(
            &bridge,
            LoopRouteRequest {
                route: &route,
                system_prompt: "system",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 2,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: request.step_id,
                semantic_request: Some(&request),
                response_facts: Some(&response_adapter),
            },
        )
        .await
        .unwrap();

        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(dispatch.message.text, "successful answer");
        let state = authority.state();
        assert_eq!(
            state.response_attempt_failures[&request.request_id][&0].failure,
            crate::session_authority::ModelResponseAttemptFailure::TransportLost
        );
        let chunks = &state.assistant_chunks[&request.request_id];
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].response_attempt_ordinal, 0);
        assert_eq!(chunks[1].response_attempt_ordinal, 1);
        let committed_id = state.request_message_commits[&request.request_id];
        let commit = &state.assistant_messages[&committed_id];
        assert_eq!(commit.response_attempt_ordinal, 1);
        assert_eq!(
            commit.content[0].chunk_refs,
            [chunks[1].content_ref.clone()]
        );
        assert!(matches!(
            state.model_requests[&request.request_id],
            crate::session_authority::ModelRequestState::Open { .. }
        ));
        assert_eq!(dispatch.response_attempt_ordinal, 1);

        let mut visible = Vec::new();
        while let Ok(event) = observer.try_recv() {
            if let omegon_traits::AgentEvent::MessageChunk { text } = event {
                visible.push(text);
            }
        }
        assert_eq!(visible, ["failed partial", "successful answer"]);
    }

    #[tokio::test]
    async fn authority_failure_append_prevents_transport_retry() {
        let (_directory, authority, scope, request) = staged_request();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let attempts = Arc::new(AtomicUsize::new(0));
        let bridge = FailAttemptAppendBridge {
            authority: authority.clone(),
            attempts: attempts.clone(),
        };
        let (events, _) = tokio::sync::broadcast::channel(4);
        let route = loop_route("anthropic", "claude-sonnet-4-6");

        let result = dispatch_loop_route(
            &bridge,
            LoopRouteRequest {
                route: &route,
                system_prompt: "system",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 2,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: request.step_id,
                semantic_request: Some(&request),
                response_facts: Some(&response_adapter),
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(
            !authority
                .state()
                .response_attempt_failures
                .contains_key(&request.request_id)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn server_retry_delay_is_scheduled_after_durable_failed_attempt() {
        let (_directory, authority, scope, request) = staged_request();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let attempts = Arc::new(AtomicUsize::new(0));
        let bridge = ServerDirectedRetryBridge {
            authority: authority.clone(),
            request_id: request.request_id,
            attempts: attempts.clone(),
        };
        let (events, mut observer) = tokio::sync::broadcast::channel(16);
        let route = loop_route("openai", "gpt-5.4");

        let dispatch = dispatch_loop_route(
            &bridge,
            LoopRouteRequest {
                route: &route,
                system_prompt: "system",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 2,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: request.step_id,
                semantic_request: Some(&request),
                response_facts: Some(&response_adapter),
            },
        )
        .await
        .unwrap();

        assert_eq!(dispatch.message.text, "recovered");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let retry_delay = std::iter::from_fn(|| observer.try_recv().ok()).find_map(|event| {
            if let omegon_traits::AgentEvent::ProviderRetry { delay_ms, .. } = event {
                Some(delay_ms)
            } else {
                None
            }
        });
        assert_eq!(retry_delay, Some(17_000));
    }

    #[tokio::test]
    async fn server_delay_does_not_make_nonretryable_failure_retryable() {
        let (_directory, _authority, scope, request) = staged_request();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let attempts = Arc::new(AtomicUsize::new(0));
        let bridge = NonRetryableServerDelayBridge {
            attempts: attempts.clone(),
        };
        let (events, _) = tokio::sync::broadcast::channel(4);
        let route = loop_route("openai", "gpt-5.4");

        let result = dispatch_loop_route(
            &bridge,
            LoopRouteRequest {
                route: &route,
                system_prompt: "system",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 2,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: request.step_id,
                semantic_request: Some(&request),
                response_facts: Some(&response_adapter),
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn server_retry_delay_falls_back_and_respects_remaining_envelope() {
        assert_eq!(select_retry_delay_ms(731, None, None), 731);
        assert_eq!(select_retry_delay_ms(731, Some(17_000), None), 17_000);
        assert_eq!(
            select_retry_delay_ms(731, Some(17_000), Some(10_000)),
            10_000
        );
    }

    #[test]
    fn retry_failure_taxonomy_is_closed_and_stable() {
        assert_eq!(
            response_attempt_failure(TransientFailureKind::ResponseIncomplete),
            crate::loop_driver::LoopResponseAttemptFailure::Eof
        );
        assert_eq!(
            response_attempt_failure(TransientFailureKind::Timeout),
            crate::loop_driver::LoopResponseAttemptFailure::TimedOut
        );
        assert_eq!(
            response_attempt_failure(TransientFailureKind::ResponseCancelled),
            crate::loop_driver::LoopResponseAttemptFailure::TransportLost
        );
        assert_eq!(
            response_attempt_failure(TransientFailureKind::ProviderOverloaded),
            crate::loop_driver::LoopResponseAttemptFailure::ProviderError
        );
    }

    #[tokio::test]
    async fn observer_sees_durable_chunk_before_broadcast() {
        let (_directory, authority, scope, request) = staged_request();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let (events, mut observer) = tokio::sync::broadcast::channel(8);
        let route = loop_route("anthropic", "claude-sonnet-4-6");
        let dispatch = dispatch_loop_route(
            &OneChunkBridge,
            LoopRouteRequest {
                route: &route,
                system_prompt: "system",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 1,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: request.step_id,
                semantic_request: Some(&request),
                response_facts: Some(&response_adapter),
            },
        );
        let observe = async {
            loop {
                if let omegon_traits::AgentEvent::MessageChunk { text } =
                    observer.recv().await.unwrap()
                {
                    let state = authority.state();
                    let chunk = state.assistant_chunks[&request.request_id].last().unwrap();
                    assert_eq!(
                        authority
                            .read_content(
                                &chunk.content_ref,
                                crate::session_authority::ProjectionClass::Default,
                            )
                            .unwrap(),
                        text.as_bytes()
                    );
                    break;
                }
            }
        };
        let (result, ()) = tokio::join!(dispatch, observe);
        result.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn durable_chunks_flush_at_size_time_and_boundaries_in_display_order() {
        let (_directory, authority, scope, request) = staged_request();
        record_loop_route_lease_for_request(
            &scope,
            request.step_id,
            "anthropic:claude-sonnet-4-6",
            "anthropic:claude-sonnet-4-6",
            Some("test"),
            Some(&request),
            None,
        )
        .unwrap();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let (events, mut observer) = tokio::sync::broadcast::channel(16);
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let consume = consume_llm_stream_with_policy(
            &mut rx,
            &events,
            "anthropic",
            "anthropic:claude-sonnet-4-6",
            None,
            StreamIdlePolicy {
                initial: Duration::from_secs(10),
                active: Duration::from_secs(10),
                reasoning: Duration::from_secs(10),
                absolute: Duration::from_secs(20),
            },
            Some(&response_adapter),
            Some(&request),
            Uuid::new_v4(),
            0,
        );
        let timed_authority = authority.clone();
        let timed_request_id = request.request_id;
        let sender = async move {
            tx.send(LlmEvent::TextDelta {
                delta: "x".repeat(MAX_DURABLE_RESPONSE_CHUNK_BYTES),
            })
            .await
            .unwrap();
            tx.send(LlmEvent::ThinkingDelta {
                delta: "think".into(),
            })
            .await
            .unwrap();
            tx.send(LlmEvent::TransportHeartbeat).await.unwrap();
            tx.send(LlmEvent::TransportHeartbeat).await.unwrap();
            tokio::time::advance(DURABLE_RESPONSE_FLUSH_INTERVAL).await;
            tokio::task::yield_now().await;
            assert!(
                timed_authority.state().assistant_chunks[&timed_request_id]
                    .iter()
                    .any(|chunk| chunk.content_kind
                        == crate::session_authority::AssistantContentKind::Thinking)
            );
            tx.send(LlmEvent::TextDelta {
                delta: "tail".into(),
            })
            .await
            .unwrap();
            tx.send(LlmEvent::Boundary {
                expectation: crate::bridge::BoundaryExpectation::Terminal,
            })
            .await
            .unwrap();
            tx.send(LlmEvent::Done {
                message: serde_json::json!({}),
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: None,
            })
            .await
            .unwrap();
        };
        let (result, ()) = tokio::join!(consume, sender);
        result.unwrap();

        let state = authority.state();
        let chunks = &state.assistant_chunks[&request.request_id];
        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks[0].content_ref.byte_length(),
            MAX_DURABLE_RESPONSE_CHUNK_BYTES as u64
        );
        assert_eq!(
            chunks[1].content_kind,
            crate::session_authority::AssistantContentKind::Thinking
        );
        assert_eq!(
            chunks[2].content_kind,
            crate::session_authority::AssistantContentKind::Text
        );
        let mut visible_kinds = Vec::new();
        while let Ok(event) = observer.try_recv() {
            match event {
                omegon_traits::AgentEvent::MessageChunk { text } => {
                    visible_kinds.push(format!("text:{}", text.len()));
                }
                omegon_traits::AgentEvent::ThinkingChunk { text } => {
                    visible_kinds.push(format!("thinking:{}", text.len()));
                }
                _ => {}
            }
        }
        assert_eq!(
            visible_kinds,
            [
                format!("text:{MAX_DURABLE_RESPONSE_CHUNK_BYTES}"),
                "thinking:5".into(),
                "text:4".into(),
            ]
        );
    }

    #[tokio::test]
    async fn eof_flushes_partial_content_but_never_commits() {
        let (_directory, authority, scope, request) = staged_request();
        record_loop_route_lease_for_request(
            &scope,
            request.step_id,
            "anthropic:claude-sonnet-4-6",
            "anthropic:claude-sonnet-4-6",
            Some("test"),
            Some(&request),
            None,
        )
        .unwrap();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let (events, _) = tokio::sync::broadcast::channel(8);
        let (tx, mut rx) = tokio::sync::mpsc::channel(2);
        tx.send(LlmEvent::TextDelta {
            delta: "partial".into(),
        })
        .await
        .unwrap();
        drop(tx);

        let result = consume_llm_stream_with_policy(
            &mut rx,
            &events,
            "anthropic",
            "anthropic:claude-sonnet-4-6",
            None,
            StreamIdlePolicy::from_env(),
            Some(&response_adapter),
            Some(&request),
            Uuid::new_v4(),
            0,
        )
        .await;

        assert!(result.is_err());
        let state = authority.state();
        assert!(
            !state
                .request_message_commits
                .contains_key(&request.request_id)
        );
        let crate::session_authority::ModelRequestState::Closed { closure, .. } =
            &state.model_requests[&request.request_id]
        else {
            panic!("EOF must close the request");
        };
        assert_eq!(
            closure.outcome,
            crate::session_authority::ModelRequestOutcome::Eof
        );
    }

    #[tokio::test]
    async fn content_append_failure_prevents_chunk_broadcast_and_commit() {
        let (_directory, authority, scope, request) = staged_request();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let bridge = FailContentAppendBridge {
            authority: authority.clone(),
        };
        let (events, mut observer) = tokio::sync::broadcast::channel(8);
        let route = loop_route("anthropic", "claude-sonnet-4-6");

        let result = dispatch_loop_route(
            &bridge,
            LoopRouteRequest {
                route: &route,
                system_prompt: "system",
                messages: &[],
                tools: &[],
                events: &events,
                max_retries: 1,
                retry_delay_ms: 1,
                cancel_keeps_prompt: None,
                scope: &scope,
                step_id: request.step_id,
                semantic_request: Some(&request),
                response_facts: Some(&response_adapter),
            },
        )
        .await;

        assert!(result.is_err());
        assert!(
            !authority
                .state()
                .request_message_commits
                .contains_key(&request.request_id)
        );
        while let Ok(event) = observer.try_recv() {
            assert!(
                !matches!(event, omegon_traits::AgentEvent::MessageChunk { .. }),
                "undurable content must not be broadcast"
            );
        }
    }

    #[tokio::test]
    async fn continuity_requires_serving_policy_and_remains_restricted() {
        let (directory, authority, scope, request) = staged_request();
        record_loop_route_lease_for_request(
            &scope,
            request.step_id,
            "anthropic:claude-sonnet-4-6",
            "anthropic:claude-sonnet-4-6",
            Some("test"),
            Some(&request),
            None,
        )
        .unwrap();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let (events, _) = tokio::sync::broadcast::channel(8);
        let (tx, mut rx) = tokio::sync::mpsc::channel(3);
        tx.send(LlmEvent::ProviderContinuity {
            kind: crate::bridge::ProviderContinuityKind::HiddenReasoning,
            bytes: b"minimum-signature".to_vec(),
        })
        .await
        .unwrap();
        tx.send(LlmEvent::TextDelta {
            delta: "answer".into(),
        })
        .await
        .unwrap();
        tx.send(LlmEvent::Done {
            message: serde_json::json!({"raw": {"must_not_be_persisted": "secret"}}),
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            provider_telemetry: None,
        })
        .await
        .unwrap();

        consume_llm_stream_with_policy(
            &mut rx,
            &events,
            "anthropic",
            "anthropic:claude-sonnet-4-6",
            None,
            StreamIdlePolicy::from_env(),
            Some(&response_adapter),
            Some(&request),
            Uuid::new_v4(),
            0,
        )
        .await
        .unwrap();

        let state = authority.state();
        let continuity = state.provider_continuity.values().next().unwrap();
        assert_eq!(continuity.request_id, request.request_id);
        assert_eq!(continuity.response_attempt_ordinal, 0);
        assert_eq!(continuity.serving_provider_id, "anthropic");
        assert_eq!(
            continuity.provider_contribution_generation_id,
            "provider:anthropic/builtin-v1"
        );
        assert!(
            authority
                .read_content(
                    &continuity.content_ref,
                    crate::session_authority::ProjectionClass::Default,
                )
                .is_err()
        );
        assert_eq!(
            authority
                .read_content(
                    &continuity.content_ref,
                    crate::session_authority::ProjectionClass::RestrictedContinuity,
                )
                .unwrap(),
            b"minimum-signature"
        );
        let authority_log =
            std::fs::read_to_string(directory.path().join("session.authority.jsonl")).unwrap();
        assert!(!authority_log.contains("must_not_be_persisted"));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_returns_without_commit_or_false_request_completion() {
        let (_directory, authority, scope, request) = staged_request();
        record_loop_route_lease_for_request(
            &scope,
            request.step_id,
            "anthropic:claude-sonnet-4-6",
            "anthropic:claude-sonnet-4-6",
            Some("test"),
            Some(&request),
            None,
        )
        .unwrap();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let (events, _) = tokio::sync::broadcast::channel(4);
        let (_tx, mut rx) = tokio::sync::mpsc::channel(1);

        let result = consume_llm_stream_with_policy(
            &mut rx,
            &events,
            "anthropic",
            "anthropic:claude-sonnet-4-6",
            None,
            StreamIdlePolicy {
                initial: Duration::from_secs(1),
                active: Duration::from_secs(1),
                reasoning: Duration::from_secs(1),
                absolute: Duration::from_secs(2),
            },
            Some(&response_adapter),
            Some(&request),
            Uuid::new_v4(),
            0,
        )
        .await;

        assert!(result.is_err());
        let state = authority.state();
        assert!(
            !state
                .request_message_commits
                .contains_key(&request.request_id)
        );
        assert!(matches!(
            state.model_requests[&request.request_id],
            crate::session_authority::ModelRequestState::Open { .. }
        ));
    }

    #[tokio::test]
    async fn cancellation_before_first_event_is_explicitly_abandoned_by_host() {
        let (_directory, authority, scope, request) = staged_request();
        record_loop_route_lease_for_request(
            &scope,
            request.step_id,
            "anthropic:claude-sonnet-4-6",
            "anthropic:claude-sonnet-4-6",
            Some("test"),
            Some(&request),
            None,
        )
        .unwrap();
        let response_adapter = crate::loop_session::LoopSemanticFactAdapter::new(&scope);
        let (events, _) = tokio::sync::broadcast::channel(4);
        let (_tx, mut rx) = tokio::sync::mpsc::channel(1);
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        let consume = consume_llm_stream_with_policy(
            &mut rx,
            &events,
            "anthropic",
            "anthropic:claude-sonnet-4-6",
            None,
            StreamIdlePolicy::from_env(),
            Some(&response_adapter),
            Some(&request),
            Uuid::new_v4(),
            0,
        );
        tokio::pin!(consume);
        tokio::select! {
            _ = cancellation.cancelled() => {}
            result = &mut consume => panic!("stream unexpectedly terminated: {result:?}"),
        }

        authority
            .terminalize_active_semantic_step(
                "2026-08-21T12:01:00Z",
                crate::session_authority::SemanticTerminalization {
                    turn_id: request.turn_id,
                    request_outcome: crate::session_authority::ModelRequestOutcome::Revoked,
                    reason_code: "operator_cancelled".into(),
                    rule_version: 1,
                },
            )
            .unwrap();

        let state = authority.state();
        assert!(
            !state
                .request_message_commits
                .contains_key(&request.request_id)
        );
        assert!(matches!(
            state.model_requests[&request.request_id],
            crate::session_authority::ModelRequestState::Closed {
                closure: crate::session_authority::ModelRequestClosed {
                    outcome: crate::session_authority::ModelRequestOutcome::Revoked,
                    response_attempt_ordinal: 0,
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            state.terminal_steps[&request.step_id],
            crate::session_authority::StepTerminalState::Abandoned { .. }
        ));
    }

    #[tokio::test]
    async fn sessionless_stream_emits_advisory_content_without_semantic_facts() {
        let (events, mut observer) = tokio::sync::broadcast::channel(8);
        let (tx, mut rx) = tokio::sync::mpsc::channel(2);
        tx.send(LlmEvent::TextDelta {
            delta: "legacy".into(),
        })
        .await
        .unwrap();
        tx.send(LlmEvent::Done {
            message: serde_json::json!({}),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            provider_telemetry: None,
        })
        .await
        .unwrap();
        let message = consume_llm_stream_with_policy(
            &mut rx,
            &events,
            "test",
            "test:model",
            None,
            StreamIdlePolicy::from_env(),
            None,
            None,
            Uuid::new_v4(),
            0,
        )
        .await
        .unwrap();
        assert_eq!(message.text, "legacy");
        assert!(matches!(
            observer.recv().await.unwrap(),
            omegon_traits::AgentEvent::MessageStart { .. }
        ));
        assert!(matches!(
            observer.recv().await.unwrap(),
            omegon_traits::AgentEvent::MessageChunk { text } if text == "legacy"
        ));
    }

    #[test]
    fn lease_preserves_selected_and_serving_route_evidence() {
        let lease =
            route_lease("openai-codex:gpt-5.5", "openai:gpt-5.5", None, None, None).unwrap();

        assert_eq!(lease.selected_provider_id, "openai-codex");
        assert_eq!(lease.serving_provider_id, "openai");
        assert_eq!(lease.selected_model_id, "gpt-5.5");
        assert_eq!(lease.serving_model_id, "gpt-5.5");
        assert_eq!(
            lease.fallback_reason.as_deref(),
            Some("selected_provider_unavailable")
        );
        assert_eq!(lease.route_policy, "declared_model_family_fallback_v1");
        assert_eq!(
            lease.contribution_generation_id,
            "provider:openai/builtin-v1"
        );
    }

    #[test]
    fn ephemeral_recorder_durably_associates_lease_with_step() {
        let dir = tempfile::tempdir().unwrap();
        let step_id = Uuid::new_v4();
        let recorder = StepRouteLeaseRecorder::at_path(step_id, dir.path().join("leases.jsonl"));
        let lease = route_lease(
            "anthropic:claude-sonnet-4-6",
            "anthropic:claude-sonnet-4-6",
            None,
            None,
            None,
        )
        .unwrap();

        recorder.record(&lease).unwrap();

        let line = std::fs::read_to_string(dir.path().join("leases.jsonl")).unwrap();
        let fact: StepRouteLeaseFact = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(fact.step_id, step_id);
        assert_eq!(fact.lease, lease);
    }

    #[test]
    fn step_recorder_never_requires_session_or_turn_identity() {
        let dir = tempfile::tempdir().unwrap();
        let recorder =
            StepRouteLeaseRecorder::at_path(Uuid::new_v4(), dir.path().join("leases.jsonl"));
        let lease = route_lease("ollama:qwen3:32b", "ollama:qwen3:32b", None, None, None).unwrap();

        recorder.record(&lease).unwrap();

        let value: serde_json::Value = serde_json::from_str(
            std::fs::read_to_string(dir.path().join("leases.jsonl"))
                .unwrap()
                .trim(),
        )
        .unwrap();
        assert!(value.get("session_id").is_none());
        assert!(value.get("turn_id").is_none());
    }

    #[tokio::test]
    async fn lease_durability_failure_blocks_provider_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let invalid_parent = dir.path().join("not-a-directory");
        std::fs::write(&invalid_parent, b"file").unwrap();
        let recorder =
            StepRouteLeaseRecorder::at_path(Uuid::new_v4(), invalid_parent.join("leases.jsonl"));
        let dispatches = Arc::new(AtomicUsize::new(0));
        let route = ResolvedProviderRoute {
            selected_model: "anthropic:claude-sonnet-4-6".into(),
            serving_model: "anthropic:claude-sonnet-4-6".into(),
            credential_source_class: "test".into(),
            native_model: "anthropic:claude-sonnet-4-6".into(),
            endpoint_provenance: None,
            admitted_capabilities: None,
            bridge: Box::new(CountingBridge(dispatches.clone())),
        };

        let result = route
            .stream(
                RouteLeaseOwner::Step(&recorder),
                "system",
                &[],
                &[],
                &StreamOptions::default(),
            )
            .await;

        assert!(result.is_err());
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stale_contribution_generation_is_rejected_before_recording() {
        let dir = tempfile::tempdir().unwrap();
        let recorder =
            StepRouteLeaseRecorder::at_path(Uuid::new_v4(), dir.path().join("leases.jsonl"));
        let mut lease = route_lease(
            "anthropic:claude-sonnet-4-6",
            "anthropic:claude-sonnet-4-6",
            None,
            None,
            None,
        )
        .unwrap();
        lease.contribution_generation_id = "provider:anthropic/stale".into();

        let error = RouteLeaseOwner::Step(&recorder).record(&lease).unwrap_err();

        assert!(error.to_string().contains("generation is stale"));
        assert!(!dir.path().join("leases.jsonl").exists());
    }

    #[test]
    fn route_retry_policy_preserves_long_reasoning_and_persistent_overload_budgets() {
        assert!(persistent_interactive_overload_retry(
            0,
            "openai-codex",
            Some(TransientFailureKind::ProviderOverloaded)
        ));
        assert!(!persistent_interactive_overload_retry(
            3,
            "openai-codex",
            Some(TransientFailureKind::ProviderOverloaded)
        ));
        assert_eq!(
            stall_exhaustion_secs("openai-codex", "gpt-5.5", Some("high")),
            2_400
        );
        assert_eq!(stall_exhaustion_secs("anthropic", "model", None), 600);
    }

    #[test]
    fn astra_reasoning_stall_budget_preserves_deep_reasoning_window() {
        for provider in ["openai", "openai-codex"] {
            for effort in ["high", "xhigh", "max"] {
                assert_eq!(
                    stall_exhaustion_secs(provider, "gpt-6-astra", Some(effort)),
                    2_400
                );
            }
        }
        assert_eq!(
            stall_exhaustion_secs("openai", "gpt-6-astra", Some("medium")),
            1_800
        );
    }

    #[test]
    fn route_failure_classification_preserves_context_repair_and_exhaustion_semantics() {
        assert_eq!(
            classify_loop_route_failure(&anyhow::anyhow!("maximum context length exceeded")),
            LoopRouteFailure::ContextOverflow
        );
        assert_eq!(
            classify_loop_route_failure(&anyhow::anyhow!(
                "tool_use ids were found without tool_result blocks"
            )),
            LoopRouteFailure::MalformedHistory
        );
        assert_eq!(
            classify_loop_route_failure(&anyhow::anyhow!(
                "upstream exhausted: retry envelope closed"
            )),
            LoopRouteFailure::Exhausted
        );
        assert_eq!(
            classify_loop_route_failure(&anyhow::anyhow!("permission denied")),
            LoopRouteFailure::Other
        );
    }

    #[test]
    fn stop_reason_normalization_only_notices_abnormal_route_stops() {
        let route = loop_route("openai", "model");
        assert!(
            provider_stop_notice(&route, &serde_json::json!({"provider_stop_reason": "stop"}))
                .is_none()
        );
        let notice = provider_stop_notice(
            &route,
            &serde_json::json!({"provider_stop_reason": "length"}),
        )
        .expect("output exhaustion is abnormal");
        assert_eq!(notice.provider, "openai");
        assert_eq!(notice.reason, "length");
        assert!(notice.message.contains("output limit was reached"));
    }

    #[tokio::test]
    async fn raw_stream_events_are_consumed_into_a_route_neutral_message() {
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(4);
        stream_tx
            .send(LlmEvent::TextDelta {
                delta: "answer".into(),
            })
            .await
            .unwrap();
        stream_tx
            .send(LlmEvent::Done {
                message: serde_json::json!({"raw": {"provider_stop_reason": "stop"}}),
                input_tokens: 11,
                output_tokens: 7,
                cache_read_tokens: 3,
                cache_creation_tokens: 2,
                provider_telemetry: None,
            })
            .await
            .unwrap();
        drop(stream_tx);
        let (events, _) = tokio::sync::broadcast::channel(16);

        let message = consume_llm_stream_with_policy(
            &mut stream_rx,
            &events,
            "test-route",
            "test-route:model",
            None,
            StreamIdlePolicy {
                initial: Duration::from_secs(1),
                active: Duration::from_secs(1),
                reasoning: Duration::from_secs(1),
                absolute: Duration::from_secs(1),
            },
            None,
            None,
            Uuid::new_v4(),
            0,
        )
        .await
        .unwrap();

        assert_eq!(message.text, "answer");
        assert_eq!(message.provider_tokens, (11, 7, 3, 2));
        assert_eq!(message.raw["provider_stop_reason"], "stop");
    }
}
