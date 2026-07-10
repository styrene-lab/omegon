//! Runtime inference inventory with provenance-preserving layer merge.
//!
//! [`ModelRegistry`](crate::model_registry::ModelRegistry) remains the embedded
//! bootstrap catalog. This module models runtime provider integrations,
//! endpoint deployments, and offerings independently, validates complete
//! snapshots, and activates them atomically with last-known-good retention.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::model_registry::{EndpointProtocol, ModelRegistry};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderIntegrationId(pub String);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointDeploymentId(pub String);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OfferingId(pub String);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConceptualModelId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InferenceInterface(pub String);

impl InferenceInterface {
    pub const CHAT_COMPLETIONS: &'static str = "chat-completions";
    pub const RESPONSES: &'static str = "responses";
    pub const ANTHROPIC_MESSAGES: &'static str = "anthropic-messages";
    pub const GEMINI_GENERATE: &'static str = "gemini-generate-content";
    pub const OLLAMA_CHAT: &'static str = "ollama-chat";
}

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
pub struct ProviderIntegration {
    pub id: ProviderIntegrationId,
    pub display_name: Evidenced<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointDeployment {
    pub id: EndpointDeploymentId,
    pub provider: Evidenced<ProviderIntegrationId>,
    pub interface: Evidenced<InferenceInterface>,
    pub base_url: Option<Evidenced<String>>,
    pub secret_refs: Evidenced<Vec<String>>,
    pub enabled: Evidenced<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceOffering {
    pub id: OfferingId,
    pub deployment: Evidenced<EndpointDeploymentId>,
    pub native_model_id: Evidenced<String>,
    pub display_name: Evidenced<String>,
    pub conceptual_model: Option<Evidenced<ConceptualModelId>>,
    pub input_modalities: Evidenced<BTreeSet<Modality>>,
    pub output_modalities: Evidenced<BTreeSet<Modality>>,
    pub capabilities: BTreeMap<String, Evidenced<bool>>,
    pub capability_grades: BTreeMap<String, Evidenced<CapabilityGrade>>,
    pub context_input: Option<Evidenced<usize>>,
    pub context_output: Option<Evidenced<usize>>,
    pub enabled: Evidenced<bool>,
}

impl InferenceOffering {
    pub fn is_graded(&self) -> bool {
        !self.capability_grades.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderPatch {
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeploymentPatch {
    pub provider: Option<ProviderIntegrationId>,
    pub interface: Option<InferenceInterface>,
    pub base_url: Option<Option<String>>,
    pub secret_refs: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OfferingPatch {
    pub deployment: Option<EndpointDeploymentId>,
    pub native_model_id: Option<String>,
    pub display_name: Option<String>,
    pub conceptual_model: Option<Option<ConceptualModelId>>,
    pub input_modalities: Option<BTreeSet<Modality>>,
    pub output_modalities: Option<BTreeSet<Modality>>,
    pub capabilities: BTreeMap<String, bool>,
    pub capability_grades: BTreeMap<String, CapabilityGrade>,
    pub context_input: Option<Option<usize>>,
    pub context_output: Option<Option<usize>>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryLayer {
    pub source: InventorySource,
    pub evidence: EvidenceKind,
    pub providers: BTreeMap<ProviderIntegrationId, ProviderPatch>,
    pub deployments: BTreeMap<EndpointDeploymentId, DeploymentPatch>,
    pub offerings: BTreeMap<OfferingId, OfferingPatch>,
}

impl InventoryLayer {
    pub fn new(source: InventorySource, evidence: EvidenceKind) -> Self {
        Self {
            source,
            evidence,
            providers: BTreeMap::new(),
            deployments: BTreeMap::new(),
            offerings: BTreeMap::new(),
        }
    }

    pub fn embedded_registry(registry: &ModelRegistry) -> Self {
        let mut layer = Self::new(InventorySource::Embedded, EvidenceKind::Declared);
        for endpoint in registry.endpoints() {
            let provider = ProviderIntegrationId(endpoint.id.clone());
            layer.providers.insert(
                provider.clone(),
                ProviderPatch {
                    display_name: Some(endpoint.display_name.clone()),
                },
            );
            layer.deployments.insert(
                EndpointDeploymentId(endpoint.id.clone()),
                DeploymentPatch {
                    provider: Some(provider),
                    interface: Some(interface_for_protocol(endpoint.protocol)),
                    base_url: Some(endpoint.base_url.clone()),
                    secret_refs: Some(
                        endpoint
                            .auth_scheme
                            .required_secret_refs()
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    ),
                    enabled: Some(endpoint.enabled),
                },
            );
        }
        for model in registry.all_models() {
            let deployment = EndpointDeploymentId(model.provider.clone());
            if !layer.deployments.contains_key(&deployment) {
                let provider = ProviderIntegrationId(model.provider.clone());
                layer.providers.entry(provider.clone()).or_insert_with(|| ProviderPatch {
                    display_name: Some(model.provider.clone()),
                });
                layer.deployments.insert(
                    deployment.clone(),
                    DeploymentPatch {
                        provider: Some(provider),
                        interface: Some(InferenceInterface("chat-completions".into())),
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
                    deployment: Some(deployment),
                    native_model_id: Some(model.id.clone()),
                    display_name: Some(model.name.clone()),
                    conceptual_model: Some(model.conceptual_model_id.clone().map(ConceptualModelId)),
                    input_modalities: Some([Modality(Modality::TEXT.into())].into()),
                    output_modalities: Some([Modality(Modality::TEXT.into())].into()),
                    capabilities,
                    capability_grades,
                    context_input: Some(Some(model.context_input)),
                    context_output: Some(Some(model.context_output)),
                    enabled: Some(true),
                },
            );
        }
        layer
    }
}

fn interface_for_protocol(protocol: EndpointProtocol) -> InferenceInterface {
    let value = match protocol {
        EndpointProtocol::OpenAiCompatible => InferenceInterface::CHAT_COMPLETIONS,
        EndpointProtocol::Anthropic => InferenceInterface::ANTHROPIC_MESSAGES,
        EndpointProtocol::GeminiNative => InferenceInterface::GEMINI_GENERATE,
        EndpointProtocol::OllamaNative => InferenceInterface::OLLAMA_CHAT,
    };
    InferenceInterface(value.into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventorySnapshot {
    pub generation: u64,
    pub providers: BTreeMap<ProviderIntegrationId, ProviderIntegration>,
    pub deployments: BTreeMap<EndpointDeploymentId, EndpointDeployment>,
    pub offerings: BTreeMap<OfferingId, InferenceOffering>,
}

impl InventorySnapshot {
    pub fn empty() -> Self {
        Self {
            generation: 0,
            providers: BTreeMap::new(),
            deployments: BTreeMap::new(),
            offerings: BTreeMap::new(),
        }
    }

    pub fn build(generation: u64, mut layers: Vec<InventoryLayer>) -> Result<Self, Vec<String>> {
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
                    ProviderIntegration {
                        id,
                        display_name: Evidenced::new(display_name, source, evidence),
                    },
                );
            } else {
                return Err(vec![format!("provider '{}' lacks display name", id.0)]);
            }
        }
        for (id, patch) in layer.deployments {
            if let Some(existing) = self.deployments.get_mut(&id) {
                if let Some(value) = patch.provider { existing.provider = Evidenced::new(value, source, evidence); }
                if let Some(value) = patch.interface { existing.interface = Evidenced::new(value, source, evidence); }
                if let Some(value) = patch.base_url { existing.base_url = value.map(|v| Evidenced::new(v, source, evidence)); }
                if let Some(value) = patch.secret_refs { existing.secret_refs = Evidenced::new(value, source, evidence); }
                if let Some(value) = patch.enabled { existing.enabled = Evidenced::new(value, source, evidence); }
            } else {
                let Some(provider) = patch.provider else { return Err(vec![format!("deployment '{}' lacks provider", id.0)]); };
                let Some(interface) = patch.interface else { return Err(vec![format!("deployment '{}' lacks interface", id.0)]); };
                self.deployments.insert(id.clone(), EndpointDeployment {
                    id,
                    provider: Evidenced::new(provider, source, evidence),
                    interface: Evidenced::new(interface, source, evidence),
                    base_url: patch.base_url.flatten().map(|v| Evidenced::new(v, source, evidence)),
                    secret_refs: Evidenced::new(patch.secret_refs.unwrap_or_default(), source, evidence),
                    enabled: Evidenced::new(patch.enabled.unwrap_or(true), source, evidence),
                });
            }
        }
        for (id, patch) in layer.offerings {
            if let Some(existing) = self.offerings.get_mut(&id) {
                apply_offering_patch(existing, patch, source, evidence);
            } else {
                self.offerings.insert(id.clone(), offering_from_patch(id, patch, source, evidence)?);
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        for deployment in self.deployments.values() {
            if !self.providers.contains_key(&deployment.provider.value) {
                errors.push(format!("deployment '{}' references unknown provider '{}'", deployment.id.0, deployment.provider.value.0));
            }
        }
        for offering in self.offerings.values() {
            if !self.deployments.contains_key(&offering.deployment.value) {
                errors.push(format!("offering '{}' references unknown deployment '{}'", offering.id.0, offering.deployment.value.0));
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    pub fn compatible_offerings(&self, request: &CompatibilityRequest) -> Vec<CompatibilityResult<'_>> {
        self.offerings.values().map(|offering| CompatibilityResult {
            offering,
            rejection_reasons: self.rejection_reasons(offering, request),
        }).collect()
    }

    fn rejection_reasons(&self, offering: &InferenceOffering, request: &CompatibilityRequest) -> Vec<RejectionReason> {
        let mut reasons = Vec::new();
        let Some(deployment) = self.deployments.get(&offering.deployment.value) else {
            return vec![RejectionReason::UnknownDeployment];
        };
        if !deployment.enabled.value || !offering.enabled.value { reasons.push(RejectionReason::Disabled); }
        if deployment.interface.value != request.interface { reasons.push(RejectionReason::InterfaceMismatch); }
        if !request.input_modalities.is_subset(&offering.input_modalities.value) { reasons.push(RejectionReason::InputModalityMismatch); }
        if !request.output_modalities.is_subset(&offering.output_modalities.value) { reasons.push(RejectionReason::OutputModalityMismatch); }
        for capability in &request.required_capabilities {
            match offering.capabilities.get(capability) {
                Some(value) if value.value && value.evidence >= request.minimum_evidence => {}
                Some(value) if value.value => reasons.push(RejectionReason::InsufficientEvidence(capability.clone())),
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
                None if !exact && !request.allow_ungraded_autonomous => {}
                None => reasons.push(RejectionReason::MissingGrade(capability.clone())),
            }
        }
        if let Some(exact_id) = &request.exact_offering {
            if exact_id != &offering.id { reasons.push(RejectionReason::NotExactOffering); }
        }
        reasons
    }
}

fn offering_from_patch(id: OfferingId, patch: OfferingPatch, source: InventorySource, evidence: EvidenceKind) -> Result<InferenceOffering, Vec<String>> {
    let Some(deployment) = patch.deployment else { return Err(vec![format!("offering '{}' lacks deployment", id.0)]); };
    let Some(native_model_id) = patch.native_model_id else { return Err(vec![format!("offering '{}' lacks native model id", id.0)]); };
    let display_name = patch.display_name.unwrap_or_else(|| native_model_id.clone());
    Ok(InferenceOffering {
        id,
        deployment: Evidenced::new(deployment, source, evidence),
        native_model_id: Evidenced::new(native_model_id, source, evidence),
        display_name: Evidenced::new(display_name, source, evidence),
        conceptual_model: patch.conceptual_model.flatten().map(|v| Evidenced::new(v, source, evidence)),
        input_modalities: Evidenced::new(patch.input_modalities.unwrap_or_default(), source, evidence),
        output_modalities: Evidenced::new(patch.output_modalities.unwrap_or_default(), source, evidence),
        capabilities: patch.capabilities.into_iter().map(|(k, v)| (k, Evidenced::new(v, source, evidence))).collect(),
        capability_grades: patch.capability_grades.into_iter().map(|(k, v)| (k, Evidenced::new(v, source, evidence))).collect(),
        context_input: patch.context_input.flatten().map(|v| Evidenced::new(v, source, evidence)),
        context_output: patch.context_output.flatten().map(|v| Evidenced::new(v, source, evidence)),
        enabled: Evidenced::new(patch.enabled.unwrap_or(true), source, evidence),
    })
}

fn apply_offering_patch(offering: &mut InferenceOffering, patch: OfferingPatch, source: InventorySource, evidence: EvidenceKind) {
    if let Some(value) = patch.deployment { offering.deployment = Evidenced::new(value, source, evidence); }
    if let Some(value) = patch.native_model_id { offering.native_model_id = Evidenced::new(value, source, evidence); }
    if let Some(value) = patch.display_name { offering.display_name = Evidenced::new(value, source, evidence); }
    if let Some(value) = patch.conceptual_model { offering.conceptual_model = value.map(|v| Evidenced::new(v, source, evidence)); }
    if let Some(value) = patch.input_modalities { offering.input_modalities = Evidenced::new(value, source, evidence); }
    if let Some(value) = patch.output_modalities { offering.output_modalities = Evidenced::new(value, source, evidence); }
    for (key, value) in patch.capabilities { offering.capabilities.insert(key, Evidenced::new(value, source, evidence)); }
    for (key, value) in patch.capability_grades { offering.capability_grades.insert(key, Evidenced::new(value, source, evidence)); }
    if let Some(value) = patch.context_input { offering.context_input = value.map(|v| Evidenced::new(v, source, evidence)); }
    if let Some(value) = patch.context_output { offering.context_output = value.map(|v| Evidenced::new(v, source, evidence)); }
    if let Some(value) = patch.enabled { offering.enabled = Evidenced::new(value, source, evidence); }
}

#[derive(Clone, Debug)]
pub struct CompatibilityRequest {
    pub interface: InferenceInterface,
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
            interface: InferenceInterface(InferenceInterface::CHAT_COMPLETIONS.into()),
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
    pub fn is_compatible(&self) -> bool { self.rejection_reasons.is_empty() }
}

#[derive(Clone, Debug)]
pub struct InferenceInventoryStore {
    active: Arc<RwLock<Arc<InventorySnapshot>>>,
}

impl InferenceInventoryStore {
    pub fn new(initial: InventorySnapshot) -> Self {
        Self { active: Arc::new(RwLock::new(Arc::new(initial))) }
    }

    pub async fn snapshot(&self) -> Arc<InventorySnapshot> {
        Arc::clone(&*self.active.read().await)
    }

    pub async fn refresh(&self, layers: Vec<InventoryLayer>) -> Result<Arc<InventorySnapshot>, Vec<String>> {
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
        layer.providers.insert(ProviderIntegrationId("lab".into()), ProviderPatch { display_name: Some("Lab".into()) });
        layer.deployments.insert(EndpointDeploymentId("lab-chat".into()), DeploymentPatch {
            provider: Some(ProviderIntegrationId("lab".into())),
            interface: Some(InferenceInterface(InferenceInterface::CHAT_COMPLETIONS.into())),
            enabled: Some(true),
            ..Default::default()
        });
        let mut grades = BTreeMap::new();
        if graded { grades.insert("agentic".into(), CapabilityGrade::A); }
        layer.offerings.insert(OfferingId("lab:model".into()), OfferingPatch {
            deployment: Some(EndpointDeploymentId("lab-chat".into())),
            native_model_id: Some("model".into()),
            display_name: Some("Internal model".into()),
            input_modalities: Some([Modality(Modality::TEXT.into())].into()),
            output_modalities: Some([Modality(output.into())].into()),
            capability_grades: grades,
            enabled: Some(true),
            ..Default::default()
        });
        layer
    }

    #[test]
    fn internal_ungraded_offering_is_valid_and_explicitly_selectable() {
        let snapshot = InventorySnapshot::build(1, vec![layer_with_offering(InventorySource::Project, Modality::TEXT, false)]).unwrap();
        let offering = snapshot.offerings.get(&OfferingId("lab:model".into())).unwrap();
        assert!(!offering.is_graded());
        let default_results = snapshot.compatible_offerings(&CompatibilityRequest::default());
        assert_eq!(default_results[0].rejection_reasons, vec![RejectionReason::UngradedNotAllowed]);
        let request = CompatibilityRequest { exact_offering: Some(offering.id.clone()), ..Default::default() };
        assert!(snapshot.compatible_offerings(&request)[0].is_compatible());
    }

    #[test]
    fn project_override_changes_one_field_and_preserves_other_provenance() {
        let base = layer_with_offering(InventorySource::Embedded, Modality::TEXT, true);
        let mut project = InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
        project.offerings.insert(OfferingId("lab:model".into()), OfferingPatch { context_input: Some(Some(99)), ..Default::default() });
        let snapshot = InventorySnapshot::build(1, vec![project, base]).unwrap();
        let offering = snapshot.offerings.get(&OfferingId("lab:model".into())).unwrap();
        assert_eq!(offering.context_input.as_ref().unwrap().source, InventorySource::Project);
        assert_eq!(offering.output_modalities.source, InventorySource::Embedded);
    }

    #[tokio::test]
    async fn invalid_refresh_retains_last_known_good_generation() {
        let initial = InventorySnapshot::build(4, vec![layer_with_offering(InventorySource::Project, Modality::TEXT, true)]).unwrap();
        let store = InferenceInventoryStore::new(initial);
        let mut invalid = InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
        invalid.offerings.insert(OfferingId("broken:model".into()), OfferingPatch {
            deployment: Some(EndpointDeploymentId("missing".into())),
            native_model_id: Some("model".into()),
            ..Default::default()
        });
        assert!(store.refresh(vec![invalid]).await.is_err());
        assert_eq!(store.snapshot().await.generation, 4);
    }

    #[tokio::test]
    async fn valid_refresh_activates_next_generation() {
        let store = InferenceInventoryStore::new(InventorySnapshot::empty());
        let activated = store.refresh(vec![layer_with_offering(InventorySource::Project, Modality::TEXT, true)]).await.unwrap();
        assert_eq!(activated.generation, 1);
        assert_eq!(store.snapshot().await.generation, 1);
    }

    #[test]
    fn modality_compatibility_precedes_grade() {
        let snapshot = InventorySnapshot::build(1, vec![layer_with_offering(InventorySource::Project, Modality::IMAGE, true)]).unwrap();
        let result = &snapshot.compatible_offerings(&CompatibilityRequest::default())[0];
        assert!(result.rejection_reasons.contains(&RejectionReason::OutputModalityMismatch));
    }

    #[test]
    fn policy_can_admit_ungraded_autonomous_offering() {
        let snapshot = InventorySnapshot::build(1, vec![layer_with_offering(InventorySource::Project, Modality::TEXT, false)]).unwrap();
        let request = CompatibilityRequest { allow_ungraded_autonomous: true, ..Default::default() };
        assert!(snapshot.compatible_offerings(&request)[0].is_compatible());
    }

    #[test]
    fn probe_evidence_does_not_invent_grade() {
        let base = layer_with_offering(InventorySource::Project, Modality::TEXT, false);
        let mut probe = InventoryLayer::new(InventorySource::Probe, EvidenceKind::Probed);
        probe.offerings.insert(OfferingId("lab:model".into()), OfferingPatch {
            capabilities: BTreeMap::from([("tools".into(), true)]),
            ..Default::default()
        });
        let snapshot = InventorySnapshot::build(1, vec![base, probe]).unwrap();
        let offering = snapshot.offerings.get(&OfferingId("lab:model".into())).unwrap();
        assert_eq!(offering.capabilities["tools"].evidence, EvidenceKind::Probed);
        assert!(offering.capability_grades.is_empty());
    }

    #[test]
    fn embedded_registry_projects_to_valid_bootstrap_inventory() {
        let layer = InventoryLayer::embedded_registry(ModelRegistry::global());
        let snapshot = InventorySnapshot::build(1, vec![layer]).unwrap();
        assert!(!snapshot.providers.is_empty());
        assert!(!snapshot.offerings.is_empty());
    }
}
