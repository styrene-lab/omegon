//! Runtime inference inventory with provenance-preserving layer merge.
//!
//! [`ModelRegistry`](crate::model_registry::ModelRegistry) remains the embedded
//! bootstrap catalog. This module models runtime provider integrations,
//! endpoint endpoints, and offerings independently, validates complete
//! snapshots, and activates them atomically with last-known-good retention.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use crate::model_registry::{EndpointProtocol, ModelRegistry};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointGroupId(pub String);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointId(pub String);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OfferingId(pub String);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConceptualModelId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConceptualModel {
    pub id: ConceptualModelId,
    pub display_name: Evidenced<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdapterId(pub String);

impl AdapterId {
    pub const CHAT_COMPLETIONS: &'static str = "chat-completions";
    pub const RESPONSES: &'static str = "responses";
    pub const ANTHROPIC_MESSAGES: &'static str = "anthropic-messages";
    pub const GEMINI_GENERATE: &'static str = "gemini-generate-content";
    pub const OLLAMA_CHAT: &'static str = "ollama-chat";
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportSpec {
    Http {
        base_url: String,
    },
    LocalProcess {
        command_ref: String,
    },
    UnixSocket {
        path: String,
    },
    /// Compatibility-only transport for endpoints still constructed by compiled clients.
    Managed,
}

pub type PolicyAttributes = BTreeMap<String, BTreeSet<String>>;
pub type ExtensionMetadata = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Modality(pub String);

impl Modality {
    pub const TEXT: &'static str = "text";
    pub const IMAGE: &'static str = "image";
    pub const VIDEO: &'static str = "video";
    pub const EMBEDDING: &'static str = "embedding";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceKind {
    Declared,
    Discovered,
    Probed,
    Benchmarked,
    RuntimeObserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InventorySource {
    Embedded,
    Organization,
    User,
    Project,
    Session,
    Discovery,
    Probe,
}

impl InventorySource {
    fn precedence(self) -> u8 {
        match self {
            Self::Embedded => 0,
            Self::Organization => 10,
            Self::User => 20,
            Self::Project => 30,
            Self::Session => 40,
            Self::Discovery => 50,
            Self::Probe => 60,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidenced<T> {
    pub value: T,
    pub source: InventorySource,
    pub evidence: EvidenceKind,
}

impl<T> Evidenced<T> {
    pub fn new(value: T, source: InventorySource, evidence: EvidenceKind) -> Self {
        Self {
            value,
            source,
            evidence,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CapabilityGrade {
    F,
    D,
    C,
    B,
    A,
    S,
}

impl CapabilityGrade {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "F" => Some(Self::F),
            "D" => Some(Self::D),
            "C" => Some(Self::C),
            "B" => Some(Self::B),
            "A" => Some(Self::A),
            "S" => Some(Self::S),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointGroup {
    pub id: EndpointGroupId,
    pub display_name: Evidenced<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceEndpoint {
    pub id: EndpointId,
    pub group: Option<Evidenced<EndpointGroupId>>,
    pub adapter: Evidenced<AdapterId>,
    pub transport: Evidenced<TransportSpec>,
    pub secret_refs: Evidenced<Vec<String>>,
    pub policy_attributes: Evidenced<PolicyAttributes>,
    pub extensions: Evidenced<ExtensionMetadata>,
    pub enabled: Evidenced<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceOffering {
    pub id: OfferingId,
    pub endpoint: Evidenced<EndpointId>,
    pub native_model_id: Evidenced<String>,
    pub display_name: Evidenced<String>,
    pub conceptual_model: Option<Evidenced<ConceptualModelId>>,
    pub input_modalities: Evidenced<BTreeSet<Modality>>,
    pub output_modalities: Evidenced<BTreeSet<Modality>>,
    pub capabilities: BTreeMap<String, Evidenced<bool>>,
    pub capability_grades: BTreeMap<String, Evidenced<CapabilityGrade>>,
    pub context_input: Option<Evidenced<usize>>,
    pub context_output: Option<Evidenced<usize>>,
    pub extensions: Evidenced<ExtensionMetadata>,
    pub enabled: Evidenced<bool>,
}

impl InferenceOffering {
    pub fn is_graded(&self) -> bool {
        !self.capability_grades.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointGroupPatch {
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConceptualModelPatch {
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointPatch {
    pub group: Option<Option<EndpointGroupId>>,
    pub adapter: Option<AdapterId>,
    pub transport: Option<TransportSpec>,
    pub secret_refs: Option<Vec<String>>,
    pub policy_attributes: Option<PolicyAttributes>,
    pub extensions: Option<ExtensionMetadata>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OfferingPatch {
    pub endpoint: Option<EndpointId>,
    pub native_model_id: Option<String>,
    pub display_name: Option<String>,
    pub conceptual_model: Option<Option<ConceptualModelId>>,
    pub input_modalities: Option<BTreeSet<Modality>>,
    pub output_modalities: Option<BTreeSet<Modality>>,
    pub capabilities: BTreeMap<String, bool>,
    pub capability_grades: BTreeMap<String, CapabilityGrade>,
    pub context_input: Option<Option<usize>>,
    pub context_output: Option<Option<usize>>,
    pub extensions: Option<ExtensionMetadata>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryLayer {
    pub source: InventorySource,
    pub evidence: EvidenceKind,
    pub providers: BTreeMap<EndpointGroupId, EndpointGroupPatch>,
    pub conceptual_models: BTreeMap<ConceptualModelId, ConceptualModelPatch>,
    pub endpoints: BTreeMap<EndpointId, EndpointPatch>,
    pub offerings: BTreeMap<OfferingId, OfferingPatch>,
}

impl InventoryLayer {
    pub fn new(source: InventorySource, evidence: EvidenceKind) -> Self {
        Self {
            source,
            evidence,
            providers: BTreeMap::new(),
            conceptual_models: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            offerings: BTreeMap::new(),
        }
    }

    pub fn embedded_registry(registry: &ModelRegistry) -> Self {
        let mut layer = Self::new(InventorySource::Embedded, EvidenceKind::Declared);
        for endpoint in registry.endpoints() {
            let provider = EndpointGroupId(endpoint.id.clone());
            layer.providers.insert(
                provider.clone(),
                EndpointGroupPatch {
                    display_name: Some(endpoint.display_name.clone()),
                },
            );
            layer.endpoints.insert(
                EndpointId(endpoint.id.clone()),
                EndpointPatch {
                    group: Some(Some(provider)),
                    adapter: Some(adapter_for_protocol(endpoint.protocol)),
                    transport: Some(
                        endpoint
                            .base_url
                            .clone()
                            .map_or(TransportSpec::Managed, |base_url| TransportSpec::Http {
                                base_url,
                            }),
                    ),
                    secret_refs: Some(
                        endpoint
                            .auth_scheme
                            .required_secret_refs()
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    ),
                    policy_attributes: Some(BTreeMap::new()),
                    extensions: Some(BTreeMap::new()),
                    enabled: Some(endpoint.enabled),
                },
            );
        }
        for model in registry.all_models() {
            let endpoint = EndpointId(model.provider.clone());
            if !layer.endpoints.contains_key(&endpoint) {
                let provider = EndpointGroupId(model.provider.clone());
                layer
                    .providers
                    .entry(provider.clone())
                    .or_insert_with(|| EndpointGroupPatch {
                        display_name: Some(model.provider.clone()),
                    });
                layer.endpoints.insert(
                    endpoint.clone(),
                    EndpointPatch {
                        group: Some(Some(provider)),
                        adapter: Some(AdapterId("chat-completions".into())),
                        transport: Some(TransportSpec::Managed),
                        enabled: Some(true),
                        ..Default::default()
                    },
                );
            }
            let mut capabilities = BTreeMap::new();
            for capability in &model.capabilities {
                capabilities.insert(capability.clone(), true);
            }
            if model.supports_reasoning {
                capabilities.insert("reasoning".into(), true);
            }
            if let Some(conceptual_model_id) = &model.conceptual_model_id {
                layer
                    .conceptual_models
                    .entry(ConceptualModelId(conceptual_model_id.clone()))
                    .or_insert_with(|| ConceptualModelPatch {
                        display_name: Some(model.name.clone()),
                    });
            }
            let mut capability_grades = BTreeMap::new();
            if let Some(grade) = registry
                .exact_grade(&model.provider, &model.id)
                .or_else(|| registry.infer_grade(&model.provider, &model.id))
                .and_then(CapabilityGrade::parse)
            {
                capability_grades.insert("agentic".into(), grade);
            }
            layer.offerings.insert(
                OfferingId(format!("{}:{}", model.provider, model.id)),
                OfferingPatch {
                    endpoint: Some(endpoint),
                    native_model_id: Some(model.id.clone()),
                    display_name: Some(model.name.clone()),
                    conceptual_model: Some(
                        model.conceptual_model_id.clone().map(ConceptualModelId),
                    ),
                    input_modalities: Some([Modality(Modality::TEXT.into())].into()),
                    output_modalities: Some([Modality(Modality::TEXT.into())].into()),
                    capabilities,
                    capability_grades,
                    context_input: Some(Some(model.context_input)),
                    context_output: Some(Some(model.context_output)),
                    extensions: Some(BTreeMap::new()),
                    enabled: Some(true),
                },
            );
        }
        layer
    }
}

fn adapter_for_protocol(protocol: EndpointProtocol) -> AdapterId {
    let value = match protocol {
        EndpointProtocol::OpenAiCompatible => AdapterId::CHAT_COMPLETIONS,
        EndpointProtocol::Anthropic => AdapterId::ANTHROPIC_MESSAGES,
        EndpointProtocol::GeminiNative => AdapterId::GEMINI_GENERATE,
        EndpointProtocol::OllamaNative => AdapterId::OLLAMA_CHAT,
    };
    AdapterId(value.into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventorySnapshot {
    pub generation: u64,
    pub providers: BTreeMap<EndpointGroupId, EndpointGroup>,
    pub conceptual_models: BTreeMap<ConceptualModelId, ConceptualModel>,
    pub endpoints: BTreeMap<EndpointId, InferenceEndpoint>,
    pub offerings: BTreeMap<OfferingId, InferenceOffering>,
}

impl InventorySnapshot {
    pub fn empty() -> Self {
        Self {
            generation: 0,
            providers: BTreeMap::new(),
            conceptual_models: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            offerings: BTreeMap::new(),
        }
    }

    pub fn build(generation: u64, mut layers: Vec<InventoryLayer>) -> Result<Self, Vec<String>> {
        if layers
            .iter()
            .any(|layer| layer.source != InventorySource::Embedded && layer_uses_managed(layer))
        {
            return Err(vec![
                "managed transport is reserved for embedded bootstrap inventory".into(),
            ]);
        }
        layers.sort_by_key(|layer| layer.source.precedence());
        let mut snapshot = Self {
            generation,
            ..Self::empty()
        };
        for layer in layers {
            snapshot.apply_layer(layer)?;
        }
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn apply_layer(&mut self, layer: InventoryLayer) -> Result<(), Vec<String>> {
        let source = layer.source;
        let evidence = layer.evidence;
        for (id, patch) in layer.providers {
            if let Some(existing) = self.providers.get_mut(&id) {
                if let Some(value) = patch.display_name {
                    existing.display_name = Evidenced::new(value, source, evidence);
                }
            } else if let Some(display_name) = patch.display_name {
                self.providers.insert(
                    id.clone(),
                    EndpointGroup {
                        id,
                        display_name: Evidenced::new(display_name, source, evidence),
                    },
                );
            } else {
                return Err(vec![format!("provider '{}' lacks display name", id.0)]);
            }
        }
        for (id, patch) in layer.conceptual_models {
            if let Some(existing) = self.conceptual_models.get_mut(&id) {
                if let Some(value) = patch.display_name {
                    existing.display_name = Evidenced::new(value, source, evidence);
                }
            } else if let Some(display_name) = patch.display_name {
                self.conceptual_models.insert(
                    id.clone(),
                    ConceptualModel {
                        id,
                        display_name: Evidenced::new(display_name, source, evidence),
                    },
                );
            } else {
                return Err(vec![format!(
                    "conceptual model '{}' lacks display name",
                    id.0
                )]);
            }
        }
        for (id, patch) in layer.endpoints {
            if let Some(existing) = self.endpoints.get_mut(&id) {
                if let Some(value) = patch.group {
                    existing.group = value.map(|v| Evidenced::new(v, source, evidence));
                }
                if let Some(value) = patch.adapter {
                    existing.adapter = Evidenced::new(value, source, evidence);
                }
                if let Some(value) = patch.transport {
                    existing.transport = Evidenced::new(value, source, evidence);
                }
                if let Some(value) = patch.secret_refs {
                    existing.secret_refs = Evidenced::new(value, source, evidence);
                }
                if let Some(value) = patch.policy_attributes {
                    existing.policy_attributes = Evidenced::new(value, source, evidence);
                }
                if let Some(value) = patch.extensions {
                    existing.extensions = Evidenced::new(value, source, evidence);
                }
                if let Some(value) = patch.enabled {
                    existing.enabled = Evidenced::new(value, source, evidence);
                }
            } else {
                let Some(adapter) = patch.adapter else {
                    return Err(vec![format!("endpoint '{}' lacks adapter", id.0)]);
                };
                let Some(transport) = patch.transport else {
                    return Err(vec![format!("endpoint '{}' lacks transport", id.0)]);
                };
                self.endpoints.insert(
                    id.clone(),
                    InferenceEndpoint {
                        id,
                        group: patch
                            .group
                            .flatten()
                            .map(|v| Evidenced::new(v, source, evidence)),
                        adapter: Evidenced::new(adapter, source, evidence),
                        transport: Evidenced::new(transport, source, evidence),
                        secret_refs: Evidenced::new(
                            patch.secret_refs.unwrap_or_default(),
                            source,
                            evidence,
                        ),
                        policy_attributes: Evidenced::new(
                            patch.policy_attributes.unwrap_or_default(),
                            source,
                            evidence,
                        ),
                        extensions: Evidenced::new(
                            patch.extensions.unwrap_or_default(),
                            source,
                            evidence,
                        ),
                        enabled: Evidenced::new(patch.enabled.unwrap_or(true), source, evidence),
                    },
                );
            }
        }
        for (id, patch) in layer.offerings {
            if let Some(existing) = self.offerings.get_mut(&id) {
                apply_offering_patch(existing, patch, source, evidence);
            } else {
                self.offerings.insert(
                    id.clone(),
                    offering_from_patch(id, patch, source, evidence)?,
                );
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        for endpoint in self.endpoints.values() {
            if let Some(group) = &endpoint.group
                && !self.providers.contains_key(&group.value)
            {
                errors.push(format!(
                    "endpoint '{}' references unknown group '{}'",
                    endpoint.id.0, group.value.0
                ));
            }
            validate_non_empty("endpoint", &endpoint.id.0, &mut errors);
            validate_non_empty("adapter", &endpoint.adapter.value.0, &mut errors);
            validate_transport(&endpoint.id, &endpoint.transport.value, &mut errors);
            validate_extensions(
                "endpoint",
                &endpoint.id.0,
                &endpoint.extensions.value,
                &mut errors,
            );
            validate_secret_refs(&endpoint.id, &endpoint.secret_refs.value, &mut errors);
        }
        for offering in self.offerings.values() {
            if !self.endpoints.contains_key(&offering.endpoint.value) {
                errors.push(format!(
                    "offering '{}' references unknown endpoint '{}'",
                    offering.id.0, offering.endpoint.value.0
                ));
            }
            validate_extensions(
                "offering",
                &offering.id.0,
                &offering.extensions.value,
                &mut errors,
            );
            if let Some(conceptual_model) = &offering.conceptual_model
                && !self.conceptual_models.contains_key(&conceptual_model.value)
            {
                errors.push(format!(
                    "offering '{}' references unknown conceptual model '{}'",
                    offering.id.0, conceptual_model.value.0
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn compatible_offerings(
        &self,
        request: &CompatibilityRequest,
    ) -> Vec<CompatibilityResult<'_>> {
        self.offerings
            .values()
            .map(|offering| CompatibilityResult {
                offering,
                rejection_reasons: self.rejection_reasons(offering, request),
            })
            .collect()
    }

    fn rejection_reasons(
        &self,
        offering: &InferenceOffering,
        request: &CompatibilityRequest,
    ) -> Vec<RejectionReason> {
        let mut reasons = Vec::new();
        let Some(endpoint) = self.endpoints.get(&offering.endpoint.value) else {
            return vec![RejectionReason::UnknownDeployment];
        };
        if !endpoint.enabled.value || !offering.enabled.value {
            reasons.push(RejectionReason::Disabled);
        }
        if endpoint.adapter.value != request.interface {
            reasons.push(RejectionReason::InterfaceMismatch);
        }
        if !request
            .input_modalities
            .is_subset(&offering.input_modalities.value)
        {
            reasons.push(RejectionReason::InputModalityMismatch);
        }
        if !request
            .output_modalities
            .is_subset(&offering.output_modalities.value)
        {
            reasons.push(RejectionReason::OutputModalityMismatch);
        }
        for capability in &request.required_capabilities {
            match offering.capabilities.get(capability) {
                Some(value) if value.value && value.evidence >= request.minimum_evidence => {}
                Some(value) if value.value => {
                    reasons.push(RejectionReason::InsufficientEvidence(capability.clone()))
                }
                _ => reasons.push(RejectionReason::MissingCapability(capability.clone())),
            }
        }
        let exact = request.exact_offering.as_ref() == Some(&offering.id);
        if !offering.is_graded() && !exact && !request.allow_ungraded_autonomous {
            reasons.push(RejectionReason::UngradedNotAllowed);
        }
        for (capability, minimum) in &request.minimum_grades {
            match offering.capability_grades.get(capability) {
                Some(actual) if actual.value >= *minimum => {}
                Some(_) => reasons.push(RejectionReason::GradeBelowMinimum(capability.clone())),
                None => reasons.push(RejectionReason::MissingGrade(capability.clone())),
            }
        }
        if let Some(exact_id) = &request.exact_offering
            && exact_id != &offering.id
        {
            reasons.push(RejectionReason::NotExactOffering);
        }
        reasons
    }
}

fn offering_from_patch(
    id: OfferingId,
    patch: OfferingPatch,
    source: InventorySource,
    evidence: EvidenceKind,
) -> Result<InferenceOffering, Vec<String>> {
    let Some(endpoint) = patch.endpoint else {
        return Err(vec![format!("offering '{}' lacks endpoint", id.0)]);
    };
    let Some(native_model_id) = patch.native_model_id else {
        return Err(vec![format!("offering '{}' lacks native model id", id.0)]);
    };
    let display_name = patch
        .display_name
        .unwrap_or_else(|| native_model_id.clone());
    Ok(InferenceOffering {
        id,
        endpoint: Evidenced::new(endpoint, source, evidence),
        native_model_id: Evidenced::new(native_model_id, source, evidence),
        display_name: Evidenced::new(display_name, source, evidence),
        conceptual_model: patch
            .conceptual_model
            .flatten()
            .map(|v| Evidenced::new(v, source, evidence)),
        input_modalities: Evidenced::new(
            patch.input_modalities.unwrap_or_default(),
            source,
            evidence,
        ),
        output_modalities: Evidenced::new(
            patch.output_modalities.unwrap_or_default(),
            source,
            evidence,
        ),
        capabilities: patch
            .capabilities
            .into_iter()
            .map(|(k, v)| (k, Evidenced::new(v, source, evidence)))
            .collect(),
        capability_grades: patch
            .capability_grades
            .into_iter()
            .map(|(k, v)| (k, Evidenced::new(v, source, evidence)))
            .collect(),
        context_input: patch
            .context_input
            .flatten()
            .map(|v| Evidenced::new(v, source, evidence)),
        context_output: patch
            .context_output
            .flatten()
            .map(|v| Evidenced::new(v, source, evidence)),
        extensions: Evidenced::new(patch.extensions.unwrap_or_default(), source, evidence),
        enabled: Evidenced::new(patch.enabled.unwrap_or(true), source, evidence),
    })
}

fn apply_offering_patch(
    offering: &mut InferenceOffering,
    patch: OfferingPatch,
    source: InventorySource,
    evidence: EvidenceKind,
) {
    if let Some(value) = patch.endpoint {
        offering.endpoint = Evidenced::new(value, source, evidence);
    }
    if let Some(value) = patch.native_model_id {
        offering.native_model_id = Evidenced::new(value, source, evidence);
    }
    if let Some(value) = patch.display_name {
        offering.display_name = Evidenced::new(value, source, evidence);
    }
    if let Some(value) = patch.conceptual_model {
        offering.conceptual_model = value.map(|v| Evidenced::new(v, source, evidence));
    }
    if let Some(value) = patch.input_modalities {
        offering.input_modalities = Evidenced::new(value, source, evidence);
    }
    if let Some(value) = patch.output_modalities {
        offering.output_modalities = Evidenced::new(value, source, evidence);
    }
    for (key, value) in patch.capabilities {
        offering
            .capabilities
            .insert(key, Evidenced::new(value, source, evidence));
    }
    for (key, value) in patch.capability_grades {
        offering
            .capability_grades
            .insert(key, Evidenced::new(value, source, evidence));
    }
    if let Some(value) = patch.context_input {
        offering.context_input = value.map(|v| Evidenced::new(v, source, evidence));
    }
    if let Some(value) = patch.context_output {
        offering.context_output = value.map(|v| Evidenced::new(v, source, evidence));
    }
    if let Some(value) = patch.extensions {
        offering.extensions = Evidenced::new(value, source, evidence);
    }
    if let Some(value) = patch.enabled {
        offering.enabled = Evidenced::new(value, source, evidence);
    }
}

fn layer_uses_managed(layer: &InventoryLayer) -> bool {
    layer
        .endpoints
        .values()
        .any(|endpoint| endpoint.transport == Some(TransportSpec::Managed))
}

fn validate_non_empty(kind: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{kind} id must not be empty"));
    }
}

fn validate_transport(id: &EndpointId, transport: &TransportSpec, errors: &mut Vec<String>) {
    let invalid = match transport {
        TransportSpec::Http { base_url } => base_url.trim().is_empty(),
        TransportSpec::LocalProcess { command_ref } => command_ref.trim().is_empty(),
        TransportSpec::UnixSocket { path } => path.trim().is_empty(),
        TransportSpec::Managed => false,
    };
    if invalid {
        errors.push(format!("endpoint '{}' has incomplete transport", id.0));
    }
}

fn validate_extensions(
    kind: &str,
    id: &str,
    extensions: &ExtensionMetadata,
    errors: &mut Vec<String>,
) {
    for key in extensions.keys() {
        if !key.contains('/') || key.starts_with('/') || key.ends_with('/') {
            errors.push(format!(
                "{kind} '{id}' has unnamespaced extension key '{key}'"
            ));
        }
    }
}

fn validate_secret_refs(id: &EndpointId, refs: &[String], errors: &mut Vec<String>) {
    for secret_ref in refs {
        let valid = !secret_ref.is_empty()
            && secret_ref
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            errors.push(format!("endpoint '{}' has invalid secret reference", id.0));
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompatibilityRequest {
    pub interface: AdapterId,
    pub input_modalities: BTreeSet<Modality>,
    pub output_modalities: BTreeSet<Modality>,
    pub required_capabilities: BTreeSet<String>,
    pub minimum_evidence: EvidenceKind,
    pub minimum_grades: BTreeMap<String, CapabilityGrade>,
    pub allow_ungraded_autonomous: bool,
    pub exact_offering: Option<OfferingId>,
}

impl Default for CompatibilityRequest {
    fn default() -> Self {
        Self {
            interface: AdapterId(AdapterId::CHAT_COMPLETIONS.into()),
            input_modalities: [Modality(Modality::TEXT.into())].into(),
            output_modalities: [Modality(Modality::TEXT.into())].into(),
            required_capabilities: BTreeSet::new(),
            minimum_evidence: EvidenceKind::Declared,
            minimum_grades: BTreeMap::new(),
            allow_ungraded_autonomous: false,
            exact_offering: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectionReason {
    UnknownDeployment,
    Disabled,
    InterfaceMismatch,
    InputModalityMismatch,
    OutputModalityMismatch,
    MissingCapability(String),
    InsufficientEvidence(String),
    UngradedNotAllowed,
    MissingGrade(String),
    GradeBelowMinimum(String),
    NotExactOffering,
}

#[derive(Clone, Debug)]
pub struct CompatibilityResult<'a> {
    pub offering: &'a InferenceOffering,
    pub rejection_reasons: Vec<RejectionReason>,
}

impl CompatibilityResult<'_> {
    pub fn is_compatible(&self) -> bool {
        self.rejection_reasons.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct InferenceInventoryStore {
    active: Arc<RwLock<Arc<InventorySnapshot>>>,
    refresh_lock: Arc<Mutex<()>>,
}

impl InferenceInventoryStore {
    pub fn new(initial: InventorySnapshot) -> Self {
        Self {
            active: Arc::new(RwLock::new(Arc::new(initial))),
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn snapshot(&self) -> Arc<InventorySnapshot> {
        Arc::clone(&*self.active.read().await)
    }

    pub async fn refresh(
        &self,
        layers: Vec<InventoryLayer>,
    ) -> Result<Arc<InventorySnapshot>, Vec<String>> {
        let _refresh_guard = self.refresh_lock.lock().await;
        let generation = self.active.read().await.generation.saturating_add(1);
        let candidate = Arc::new(InventorySnapshot::build(generation, layers)?);
        *self.active.write().await = Arc::clone(&candidate);
        Ok(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer_with_offering(source: InventorySource, output: &str, graded: bool) -> InventoryLayer {
        let mut layer = InventoryLayer::new(source, EvidenceKind::Declared);
        layer.providers.insert(
            EndpointGroupId("lab".into()),
            EndpointGroupPatch {
                display_name: Some("Lab".into()),
            },
        );
        layer.endpoints.insert(
            EndpointId("lab-chat".into()),
            EndpointPatch {
                group: Some(Some(EndpointGroupId("lab".into()))),
                adapter: Some(AdapterId(AdapterId::CHAT_COMPLETIONS.into())),
                transport: Some(TransportSpec::Http {
                    base_url: "https://lab.example/v1".into(),
                }),
                enabled: Some(true),
                ..Default::default()
            },
        );
        let mut grades = BTreeMap::new();
        if graded {
            grades.insert("agentic".into(), CapabilityGrade::A);
        }
        layer.offerings.insert(
            OfferingId("lab:model".into()),
            OfferingPatch {
                endpoint: Some(EndpointId("lab-chat".into())),
                native_model_id: Some("model".into()),
                display_name: Some("Internal model".into()),
                input_modalities: Some([Modality(Modality::TEXT.into())].into()),
                output_modalities: Some([Modality(output.into())].into()),
                capability_grades: grades,
                enabled: Some(true),
                ..Default::default()
            },
        );
        layer
    }

    #[test]
    fn standalone_private_endpoint_requires_no_group() {
        let mut layer = layer_with_offering(InventorySource::Project, Modality::TEXT, true);
        let endpoint = layer
            .endpoints
            .get_mut(&EndpointId("lab-chat".into()))
            .unwrap();
        endpoint.group = Some(None);
        endpoint.transport = Some(TransportSpec::Http {
            base_url: "https://inference.internal.example/v1".into(),
        });
        endpoint.policy_attributes = Some(BTreeMap::from([
            ("network".into(), BTreeSet::from(["private".into()])),
            ("operator".into(), BTreeSet::from(["organization".into()])),
        ]));
        layer.providers.clear();
        let snapshot = InventorySnapshot::build(1, vec![layer]).unwrap();
        assert!(snapshot.providers.is_empty());
        assert!(
            snapshot.endpoints[&EndpointId("lab-chat".into())]
                .group
                .is_none()
        );
    }

    #[test]
    fn extension_metadata_is_opaque_to_compatibility() {
        let base = layer_with_offering(InventorySource::Project, Modality::TEXT, true);
        let baseline = InventorySnapshot::build(1, vec![base.clone()]).unwrap();
        let mut enriched = base;
        enriched
            .offerings
            .get_mut(&OfferingId("lab:model".into()))
            .unwrap()
            .extensions = Some(BTreeMap::from([(
            "connector.example/deployment".into(),
            "opaque-42".into(),
        )]));
        let enriched = InventorySnapshot::build(1, vec![enriched]).unwrap();
        assert_eq!(
            baseline.compatible_offerings(&CompatibilityRequest::default())[0].rejection_reasons,
            enriched.compatible_offerings(&CompatibilityRequest::default())[0].rejection_reasons
        );
    }

    #[test]
    fn malformed_transport_secret_ref_and_extension_are_rejected() {
        let mut layer = layer_with_offering(InventorySource::Project, Modality::TEXT, true);
        let endpoint = layer
            .endpoints
            .get_mut(&EndpointId("lab-chat".into()))
            .unwrap();
        endpoint.transport = Some(TransportSpec::Http {
            base_url: String::new(),
        });
        endpoint.secret_refs = Some(vec!["sk-live-secret-value".into()]);
        endpoint.extensions = Some(BTreeMap::from([("unnamespaced".into(), "x".into())]));
        let errors = InventorySnapshot::build(1, vec![layer]).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("incomplete transport"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("invalid secret reference"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("unnamespaced extension"))
        );
    }

    #[test]
    fn managed_transport_is_rejected_outside_embedded_bootstrap() {
        let mut layer = layer_with_offering(InventorySource::Project, Modality::TEXT, true);
        layer
            .endpoints
            .get_mut(&EndpointId("lab-chat".into()))
            .unwrap()
            .transport = Some(TransportSpec::Managed);
        let errors = InventorySnapshot::build(1, vec![layer]).unwrap_err();
        assert!(errors[0].contains("reserved for embedded bootstrap"));
    }

    #[test]
    fn internal_ungraded_offering_is_valid_and_explicitly_selectable() {
        let snapshot = InventorySnapshot::build(
            1,
            vec![layer_with_offering(
                InventorySource::Project,
                Modality::TEXT,
                false,
            )],
        )
        .unwrap();
        let offering = snapshot
            .offerings
            .get(&OfferingId("lab:model".into()))
            .unwrap();
        assert!(!offering.is_graded());
        let default_results = snapshot.compatible_offerings(&CompatibilityRequest::default());
        assert_eq!(
            default_results[0].rejection_reasons,
            vec![RejectionReason::UngradedNotAllowed]
        );
        let request = CompatibilityRequest {
            exact_offering: Some(offering.id.clone()),
            ..Default::default()
        };
        assert!(snapshot.compatible_offerings(&request)[0].is_compatible());
    }

    #[test]
    fn project_override_changes_one_field_and_preserves_other_provenance() {
        let base = layer_with_offering(InventorySource::Embedded, Modality::TEXT, true);
        let mut project = InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
        project.offerings.insert(
            OfferingId("lab:model".into()),
            OfferingPatch {
                context_input: Some(Some(99)),
                ..Default::default()
            },
        );
        let snapshot = InventorySnapshot::build(1, vec![project, base]).unwrap();
        let offering = snapshot
            .offerings
            .get(&OfferingId("lab:model".into()))
            .unwrap();
        assert_eq!(
            offering.context_input.as_ref().unwrap().source,
            InventorySource::Project
        );
        assert_eq!(offering.output_modalities.source, InventorySource::Embedded);
    }

    #[tokio::test]
    async fn invalid_refresh_retains_last_known_good_generation() {
        let initial = InventorySnapshot::build(
            4,
            vec![layer_with_offering(
                InventorySource::Project,
                Modality::TEXT,
                true,
            )],
        )
        .unwrap();
        let store = InferenceInventoryStore::new(initial);
        let mut invalid = InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
        invalid.offerings.insert(
            OfferingId("broken:model".into()),
            OfferingPatch {
                endpoint: Some(EndpointId("missing".into())),
                native_model_id: Some("model".into()),
                ..Default::default()
            },
        );
        assert!(store.refresh(vec![invalid]).await.is_err());
        assert_eq!(store.snapshot().await.generation, 4);
    }

    #[tokio::test]
    async fn valid_refresh_activates_next_generation() {
        let store = InferenceInventoryStore::new(InventorySnapshot::empty());
        let activated = store
            .refresh(vec![layer_with_offering(
                InventorySource::Project,
                Modality::TEXT,
                true,
            )])
            .await
            .unwrap();
        assert_eq!(activated.generation, 1);
        assert_eq!(store.snapshot().await.generation, 1);
    }

    #[test]
    fn modality_compatibility_precedes_grade() {
        let snapshot = InventorySnapshot::build(
            1,
            vec![layer_with_offering(
                InventorySource::Project,
                Modality::IMAGE,
                true,
            )],
        )
        .unwrap();
        let result = &snapshot.compatible_offerings(&CompatibilityRequest::default())[0];
        assert!(
            result
                .rejection_reasons
                .contains(&RejectionReason::OutputModalityMismatch)
        );
    }

    #[test]
    fn policy_can_admit_ungraded_autonomous_offering() {
        let snapshot = InventorySnapshot::build(
            1,
            vec![layer_with_offering(
                InventorySource::Project,
                Modality::TEXT,
                false,
            )],
        )
        .unwrap();
        let request = CompatibilityRequest {
            allow_ungraded_autonomous: true,
            ..Default::default()
        };
        assert!(snapshot.compatible_offerings(&request)[0].is_compatible());
    }

    #[test]
    fn probe_evidence_does_not_invent_grade() {
        let base = layer_with_offering(InventorySource::Project, Modality::TEXT, false);
        let mut probe = InventoryLayer::new(InventorySource::Probe, EvidenceKind::Probed);
        probe.offerings.insert(
            OfferingId("lab:model".into()),
            OfferingPatch {
                capabilities: BTreeMap::from([("tools".into(), true)]),
                ..Default::default()
            },
        );
        let snapshot = InventorySnapshot::build(1, vec![base, probe]).unwrap();
        let offering = snapshot
            .offerings
            .get(&OfferingId("lab:model".into()))
            .unwrap();
        assert_eq!(
            offering.capabilities["tools"].evidence,
            EvidenceKind::Probed
        );
        assert!(offering.capability_grades.is_empty());
    }

    #[test]
    fn grade_floor_is_hard_even_when_ungraded_routes_are_admitted() {
        let snapshot = InventorySnapshot::build(
            1,
            vec![layer_with_offering(
                InventorySource::Project,
                Modality::TEXT,
                false,
            )],
        )
        .unwrap();
        let request = CompatibilityRequest {
            allow_ungraded_autonomous: true,
            minimum_grades: BTreeMap::from([("agentic".into(), CapabilityGrade::B)]),
            ..Default::default()
        };
        assert_eq!(
            snapshot.compatible_offerings(&request)[0].rejection_reasons,
            vec![RejectionReason::MissingGrade("agentic".into())]
        );
    }

    #[test]
    fn dangling_conceptual_model_reference_is_rejected() {
        let mut layer = layer_with_offering(InventorySource::Project, Modality::TEXT, true);
        layer
            .offerings
            .get_mut(&OfferingId("lab:model".into()))
            .unwrap()
            .conceptual_model = Some(Some(ConceptualModelId("missing".into())));
        let errors = InventorySnapshot::build(1, vec![layer]).unwrap_err();
        assert!(errors[0].contains("unknown conceptual model 'missing'"));
    }

    #[tokio::test]
    async fn concurrent_refreshes_receive_distinct_generations() {
        let store = InferenceInventoryStore::new(InventorySnapshot::empty());
        let first = store.refresh(vec![layer_with_offering(
            InventorySource::Project,
            Modality::TEXT,
            true,
        )]);
        let second = store.refresh(vec![layer_with_offering(
            InventorySource::Project,
            Modality::TEXT,
            true,
        )]);
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap().generation, 1);
        assert_eq!(second.unwrap().generation, 2);
        assert_eq!(store.snapshot().await.generation, 2);
    }

    #[test]
    fn embedded_registry_projects_to_valid_bootstrap_inventory() {
        let layer = InventoryLayer::embedded_registry(ModelRegistry::global());
        let snapshot = InventorySnapshot::build(1, vec![layer]).unwrap();
        assert!(!snapshot.providers.is_empty());
        assert!(!snapshot.offerings.is_empty());
    }
}
