use std::sync::Arc;

use crate::inference_inventory::{
    CapabilityGrade, CompatibilityRequest, InferenceInventoryStore, InventoryLayer,
    InventorySnapshot,
};
use crate::inference_manifest::{InferenceManifestLoader, ManifestDiagnostic, ManifestSource};
use crate::model_registry::ModelRegistry;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InventoryRoutePolicy {
    #[default]
    Shadow,
    Prefer,
}

impl InventoryRoutePolicy {
    pub fn from_env() -> Self {
        match std::env::var("OMEGON_INFERENCE_ROUTE_POLICY") {
            Ok(value) if value.eq_ignore_ascii_case("inventory_prefer") => Self::Prefer,
            _ => Self::Shadow,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryRoutePreference {
    pub offering: String,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceRefreshReport {
    pub previous_generation: u64,
    pub active_generation: u64,
    pub activated: bool,
    pub loaded_sources: Vec<ManifestSource>,
    pub endpoint_count: usize,
    pub offering_count: usize,
    pub diagnostics: Vec<ManifestDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteShadowAgreement {
    ExactOffering,
    ConceptualModel,
    Provider,
    Divergence,
    NoAuthoritativeCandidate,
    NoInventoryCandidate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteShadowObservation {
    pub generation: u64,
    pub authoritative: Option<String>,
    pub inventory_candidates: Vec<String>,
    pub agreement: RouteShadowAgreement,
}

impl RouteShadowObservation {
    fn divergence_detail(&self) -> Option<String> {
        (self.agreement == RouteShadowAgreement::Divergence).then(|| {
            format!(
                "{} != {}",
                self.authoritative.as_deref().unwrap_or("none"),
                self.inventory_candidates
                    .first()
                    .map(String::as_str)
                    .unwrap_or("none")
            )
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteShadowSummary {
    pub observations: usize,
    pub exact_matches: usize,
    pub conceptual_matches: usize,
    pub provider_matches: usize,
    pub divergences: usize,
    pub no_authoritative_candidate: usize,
    pub no_inventory_candidate: usize,
    pub recent_divergences: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceRuntimeProjection {
    pub generation: u64,
    pub active_sources: Vec<ManifestSource>,
    pub endpoint_count: usize,
    pub offering_count: usize,
    pub last_rejected_diagnostics: Vec<ManifestDiagnostic>,
    pub route_shadow_observations: Vec<RouteShadowObservation>,
}

impl InferenceRuntimeProjection {
    pub fn route_shadow_summary(&self) -> RouteShadowSummary {
        let mut summary = summarize_observations(self.route_shadow_observations.iter());
        summary.recent_divergences = self
            .route_shadow_observations
            .iter()
            .rev()
            .filter_map(RouteShadowObservation::divergence_detail)
            .take(3)
            .collect();
        summary
    }

    pub fn render_text(&self) -> String {
        let mut output = format!(
            "Inference inventory\nGeneration: {}\nEndpoints: {}\nOfferings: {}\nActive manifest sources: {}",
            self.generation,
            self.endpoint_count,
            self.offering_count,
            self.active_sources.len()
        );
        if !self.last_rejected_diagnostics.is_empty() {
            output.push_str("\nLast rejected refresh:");
            for diagnostic in &self.last_rejected_diagnostics {
                output.push_str(&format!(
                    "\n- {:?} {}: {}",
                    diagnostic.phase,
                    diagnostic.path.display(),
                    diagnostic.message
                ));
            }
        }
        let summary = self.route_shadow_summary();
        let current_observations: Vec<_> = self
            .route_shadow_observations
            .iter()
            .filter(|observation| observation.generation == self.generation)
            .collect();
        let current_summary = summarize_observations(current_observations.iter().copied());
        output.push_str(&format!(
            "\nRoute shadow: {} observations; {} exact, {} conceptual, {} provider, {} divergent, {} without authoritative candidate, {} without inventory candidate. Current generation {}: {} observations, {:.1}% normalized parity; authority readiness: {}",
            summary.observations,
            summary.exact_matches,
            summary.conceptual_matches,
            summary.provider_matches,
            summary.divergences,
            summary.no_authoritative_candidate,
            summary.no_inventory_candidate,
            self.generation,
            current_summary.observations,
            current_summary.normalized_parity_percent(),
            current_summary.readiness().label(),
        ));
        if !summary.recent_divergences.is_empty() {
            output.push_str("\nRecent route divergences:");
            for detail in summary.recent_divergences {
                output.push_str(&format!("\n- {detail}"));
            }
        }
        output
    }
}

fn summarize_observations<'a>(
    observations: impl Iterator<Item = &'a RouteShadowObservation>,
) -> RouteShadowSummary {
    let mut summary = RouteShadowSummary::default();
    for observation in observations {
        summary.observations += 1;
        match observation.agreement {
            RouteShadowAgreement::ExactOffering => summary.exact_matches += 1,
            RouteShadowAgreement::ConceptualModel => summary.conceptual_matches += 1,
            RouteShadowAgreement::Provider => summary.provider_matches += 1,
            RouteShadowAgreement::Divergence => summary.divergences += 1,
            RouteShadowAgreement::NoAuthoritativeCandidate => {
                summary.no_authoritative_candidate += 1;
            }
            RouteShadowAgreement::NoInventoryCandidate => summary.no_inventory_candidate += 1,
        }
    }
    summary
}

impl RouteShadowSummary {
    fn normalized_parity_percent(&self) -> f64 {
        if self.observations == 0 {
            return 0.0;
        }
        100.0 * (self.exact_matches + self.conceptual_matches + self.provider_matches) as f64
            / self.observations as f64
    }

    fn readiness(&self) -> RouteAuthorityReadiness {
        const MINIMUM_OBSERVATIONS: usize = 20;
        const MINIMUM_PARITY_PERCENT: f64 = 95.0;
        if self.observations < MINIMUM_OBSERVATIONS {
            RouteAuthorityReadiness::InsufficientEvidence
        } else if self.normalized_parity_percent() < MINIMUM_PARITY_PERCENT {
            RouteAuthorityReadiness::BelowParityThreshold
        } else {
            RouteAuthorityReadiness::ReadyForReview
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RouteAuthorityReadiness {
    InsufficientEvidence,
    BelowParityThreshold,
    ReadyForReview,
}

impl RouteAuthorityReadiness {
    fn label(self) -> &'static str {
        match self {
            Self::InsufficientEvidence => "insufficient-evidence",
            Self::BelowParityThreshold => "below-threshold",
            Self::ReadyForReview => "ready-for-review",
        }
    }
}

fn normalize_route_id(route: &str) -> String {
    let Some((provider, model)) = route.split_once(':') else {
        return route.to_ascii_lowercase();
    };
    let provider = provider.to_ascii_lowercase();
    let provider = match provider.as_str() {
        "local" => "ollama",
        "copilot" => "github-copilot",
        other => other,
    };
    let model = model.to_ascii_lowercase();
    let model = model.strip_suffix(":latest").unwrap_or(&model);
    format!("{provider}:{model}")
}

fn classify_route_shadow(
    snapshot: &InventorySnapshot,
    authoritative: Option<&str>,
    candidates: &[String],
) -> RouteShadowAgreement {
    let Some(authoritative) = authoritative else {
        return RouteShadowAgreement::NoAuthoritativeCandidate;
    };
    if candidates.is_empty() {
        return RouteShadowAgreement::NoInventoryCandidate;
    }
    let authoritative_normalized = normalize_route_id(authoritative);
    if candidates
        .iter()
        .any(|candidate| normalize_route_id(candidate) == authoritative_normalized)
    {
        return RouteShadowAgreement::ExactOffering;
    }
    let authoritative_offering = snapshot
        .offerings
        .get(&crate::inference_inventory::OfferingId(
            authoritative.to_owned(),
        ));
    if let Some(authoritative_model) = authoritative_offering
        .and_then(|offering| offering.conceptual_model.as_ref())
        .map(|model| &model.value)
        && candidates.iter().any(|candidate| {
            snapshot
                .offerings
                .get(&crate::inference_inventory::OfferingId(candidate.clone()))
                .and_then(|offering| offering.conceptual_model.as_ref())
                .is_some_and(|model| &model.value == authoritative_model)
        })
    {
        return RouteShadowAgreement::ConceptualModel;
    }
    let authoritative_provider = authoritative_normalized
        .split_once(':')
        .map(|(provider, _)| provider);
    if authoritative_provider.is_some_and(|provider| {
        candidates.iter().any(|candidate| {
            normalize_route_id(candidate)
                .split_once(':')
                .is_some_and(|(candidate_provider, _)| candidate_provider == provider)
        })
    }) {
        return RouteShadowAgreement::Provider;
    }
    RouteShadowAgreement::Divergence
}

fn provider_allowed(offering: &str, only_providers: &[String]) -> bool {
    only_providers.is_empty()
        || only_providers.iter().any(|provider| {
            normalize_route_id(&format!("{provider}:placeholder"))
                .split_once(':')
                .is_some_and(|(normalized, _)| {
                    normalize_route_id(offering)
                        .split_once(':')
                        .is_some_and(|(candidate, _)| candidate == normalized)
                })
        })
}

fn route_supported_by_compiled_bridge(offering: &str) -> bool {
    normalize_route_id(offering)
        .split_once(':')
        .is_some_and(|(provider, _)| {
            matches!(
                provider,
                "anthropic"
                    | "openai"
                    | "openai-codex"
                    | "github-copilot"
                    | "google"
                    | "google-antigravity"
                    | "ollama"
                    | "openrouter"
                    | "xai"
                    | "mistral"
                    | "huggingface"
            )
        })
}

fn shadow_grade_floor(grade: crate::routing::CapabilityGradeBand) -> Option<CapabilityGrade> {
    match grade {
        crate::routing::CapabilityGradeBand::Max => Some(CapabilityGrade::S),
        crate::routing::CapabilityGradeBand::Frontier => Some(CapabilityGrade::B),
        crate::routing::CapabilityGradeBand::Mid => Some(CapabilityGrade::D),
        crate::routing::CapabilityGradeBand::Leaf => None,
    }
}

#[derive(Clone, Debug)]
pub struct InferenceRuntimeState {
    store: InferenceInventoryStore,
    loader: InferenceManifestLoader,
    sources: Vec<ManifestSource>,
    active_sources: Arc<tokio::sync::RwLock<Vec<ManifestSource>>>,
    last_rejected_diagnostics: Arc<tokio::sync::RwLock<Vec<ManifestDiagnostic>>>,
    route_shadow_observations: Arc<tokio::sync::RwLock<Vec<RouteShadowObservation>>>,
}

impl InferenceRuntimeState {
    pub fn new(project_root: &std::path::Path) -> Self {
        let embedded = InventoryLayer::embedded_registry(ModelRegistry::global());
        let initial = InventorySnapshot::build(1, vec![embedded.clone()])
            .expect("embedded inference registry must project to a valid inventory");
        let home = crate::paths::omegon_home().unwrap_or_else(|_| project_root.join(".omegon"));
        let sources = InferenceManifestLoader::default_sources(&home, project_root);
        Self::with_runtime_parts(initial, embedded, sources)
    }

    fn with_runtime_parts(
        initial: InventorySnapshot,
        embedded: InventoryLayer,
        sources: Vec<ManifestSource>,
    ) -> Self {
        Self {
            store: InferenceInventoryStore::new(initial),
            loader: InferenceManifestLoader::new(embedded, sources.clone()),
            sources,
            active_sources: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            last_rejected_diagnostics: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            route_shadow_observations: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    #[cfg(test)]
    pub fn with_parts(
        initial: InventorySnapshot,
        embedded: InventoryLayer,
        sources: Vec<ManifestSource>,
    ) -> Self {
        Self::with_runtime_parts(initial, embedded, sources)
    }

    pub async fn snapshot(&self) -> Arc<InventorySnapshot> {
        self.store.snapshot().await
    }

    pub async fn projection(&self) -> InferenceRuntimeProjection {
        let snapshot = self.store.snapshot().await;
        InferenceRuntimeProjection {
            generation: snapshot.generation,
            active_sources: self.active_sources.read().await.clone(),
            endpoint_count: snapshot.endpoints.len(),
            offering_count: snapshot.offerings.len(),
            last_rejected_diagnostics: self.last_rejected_diagnostics.read().await.clone(),
            route_shadow_observations: self.route_shadow_observations.read().await.clone(),
        }
    }

    pub async fn preferred_route(
        &self,
        grade: crate::routing::CapabilityGradeBand,
        only_providers: &[String],
        exact_offering: Option<&str>,
    ) -> Option<InventoryRoutePreference> {
        if InventoryRoutePolicy::from_env() != InventoryRoutePolicy::Prefer {
            return None;
        }
        let projection = self.projection().await;
        let current: Vec<_> = projection
            .route_shadow_observations
            .iter()
            .filter(|observation| observation.generation == projection.generation)
            .collect();
        if summarize_observations(current.into_iter()).readiness()
            != RouteAuthorityReadiness::ReadyForReview
        {
            return None;
        }
        let snapshot = self.store.snapshot().await;
        let mut request = CompatibilityRequest {
            exact_offering: exact_offering
                .map(|id| crate::inference_inventory::OfferingId(normalize_route_id(id))),
            ..Default::default()
        };
        if let Some(minimum) = shadow_grade_floor(grade) {
            request.minimum_grades.insert("agentic".into(), minimum);
        }
        let offering = snapshot
            .compatible_offerings(&request)
            .into_iter()
            .find(|result| {
                result.is_compatible()
                    && provider_allowed(&result.offering.id.0, only_providers)
                    && route_supported_by_compiled_bridge(&result.offering.id.0)
            })?;
        Some(InventoryRoutePreference {
            offering: offering.offering.id.0.clone(),
            generation: snapshot.generation,
        })
    }

    pub async fn observe_route_shadow(
        &self,
        grade: crate::routing::CapabilityGradeBand,
        authoritative: Option<&str>,
        only_providers: &[String],
        exact_offering: Option<&str>,
    ) -> RouteShadowObservation {
        let snapshot = self.store.snapshot().await;
        let mut request = CompatibilityRequest {
            exact_offering: exact_offering
                .map(|id| crate::inference_inventory::OfferingId(normalize_route_id(id))),
            ..Default::default()
        };
        if let Some(minimum) = shadow_grade_floor(grade) {
            request.minimum_grades.insert("agentic".into(), minimum);
        }
        let inventory_candidates: Vec<String> = snapshot
            .compatible_offerings(&request)
            .into_iter()
            .filter(|result| result.is_compatible())
            .filter(|result| provider_allowed(&result.offering.id.0, only_providers))
            .map(|result| result.offering.id.0.clone())
            .collect();
        let authoritative = authoritative.map(str::to_owned);
        let agreement =
            classify_route_shadow(&snapshot, authoritative.as_deref(), &inventory_candidates);
        let observation = RouteShadowObservation {
            generation: snapshot.generation,
            authoritative,
            inventory_candidates,
            agreement,
        };
        let mut observations = self.route_shadow_observations.write().await;
        observations.push(observation.clone());
        if observations.len() > 64 {
            let excess = observations.len() - 64;
            observations.drain(..excess);
        }
        observation
    }

    pub async fn inventory_route_preference(
        &self,
        grade: crate::routing::CapabilityGradeBand,
        only_providers: &[String],
        exact_offering: Option<&str>,
    ) -> Option<InventoryRoutePreference> {
        if InventoryRoutePolicy::from_env() != InventoryRoutePolicy::Prefer {
            return None;
        }
        let snapshot = self.store.snapshot().await;
        let observations = self.route_shadow_observations.read().await;
        let current = summarize_observations(
            observations
                .iter()
                .filter(|observation| observation.generation == snapshot.generation),
        );
        if current.readiness() != RouteAuthorityReadiness::ReadyForReview {
            return None;
        }
        let mut request = CompatibilityRequest {
            exact_offering: exact_offering
                .map(|id| crate::inference_inventory::OfferingId(normalize_route_id(id))),
            ..Default::default()
        };
        if let Some(minimum) = shadow_grade_floor(grade) {
            request.minimum_grades.insert("agentic".into(), minimum);
        }
        snapshot
            .compatible_offerings(&request)
            .into_iter()
            .find(|result| {
                result.is_compatible()
                    && provider_allowed(&result.offering.id.0, only_providers)
                    && route_supported_by_compiled_bridge(&result.offering.id.0)
            })
            .map(|result| InventoryRoutePreference {
                offering: normalize_route_id(&result.offering.id.0),
                generation: snapshot.generation,
            })
    }

    pub async fn record_refresh_report(&self, report: &InferenceRefreshReport) {
        let diagnostics = if report.activated {
            Vec::new()
        } else {
            report.diagnostics.clone()
        };
        *self.last_rejected_diagnostics.write().await = diagnostics;
    }

    pub async fn refresh(&self) -> InferenceRefreshReport {
        let previous = self.store.snapshot().await;
        match self.loader.reload(&self.store).await {
            Ok(_) => {
                let active = self.store.snapshot().await;
                let loaded_sources: Vec<_> = self
                    .sources
                    .iter()
                    .filter(|source| source.path.is_file())
                    .cloned()
                    .collect();
                *self.active_sources.write().await = loaded_sources.clone();
                InferenceRefreshReport {
                    previous_generation: previous.generation,
                    active_generation: active.generation,
                    activated: true,
                    loaded_sources,
                    endpoint_count: active.endpoints.len(),
                    offering_count: active.offerings.len(),
                    diagnostics: Vec::new(),
                }
            }
            Err(diagnostics) => InferenceRefreshReport {
                previous_generation: previous.generation,
                active_generation: previous.generation,
                activated: false,
                loaded_sources: self.active_sources.read().await.clone(),
                endpoint_count: previous.endpoints.len(),
                offering_count: previous.offerings.len(),
                diagnostics,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_all_route_relationships() {
        let snapshot = InventorySnapshot::empty();
        assert_eq!(
            classify_route_shadow(&snapshot, Some("openai:gpt"), &["openai:gpt".into()]),
            RouteShadowAgreement::ExactOffering
        );
        assert_eq!(
            classify_route_shadow(&snapshot, Some("openai:gpt"), &["openai:other".into()]),
            RouteShadowAgreement::Provider
        );
        assert_eq!(
            classify_route_shadow(&snapshot, Some("anthropic:claude"), &["openai:gpt".into()]),
            RouteShadowAgreement::Divergence
        );
        assert_eq!(
            classify_route_shadow(&snapshot, None, &["openai:gpt".into()]),
            RouteShadowAgreement::NoAuthoritativeCandidate
        );
        assert_eq!(
            classify_route_shadow(&snapshot, Some("openai:gpt"), &[]),
            RouteShadowAgreement::NoInventoryCandidate
        );
    }

    #[test]
    fn readiness_requires_volume_and_parity() {
        let observation = |agreement| RouteShadowObservation {
            generation: 2,
            authoritative: None,
            inventory_candidates: Vec::new(),
            agreement,
        };
        let insufficient = vec![observation(RouteShadowAgreement::ExactOffering); 19];
        assert_eq!(
            summarize_observations(insufficient.iter()).readiness(),
            RouteAuthorityReadiness::InsufficientEvidence
        );
        let ready = vec![observation(RouteShadowAgreement::ExactOffering); 20];
        assert_eq!(
            summarize_observations(ready.iter()).readiness(),
            RouteAuthorityReadiness::ReadyForReview
        );
        let mut below = ready;
        below[0].agreement = RouteShadowAgreement::Divergence;
        below[1].agreement = RouteShadowAgreement::Divergence;
        assert_eq!(
            summarize_observations(below.iter()).readiness(),
            RouteAuthorityReadiness::BelowParityThreshold
        );
    }

    #[test]
    fn route_normalization_handles_aliases_and_latest() {
        assert_eq!(normalize_route_id("local:QWEN:latest"), "ollama:qwen");
        assert_eq!(
            normalize_route_id("copilot:GPT-5.4"),
            "github-copilot:gpt-5.4"
        );
    }
}
