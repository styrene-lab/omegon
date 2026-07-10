use std::sync::Arc;

use crate::inference_inventory::{
    CapabilityGrade, CompatibilityRequest, InferenceInventoryStore, InventoryLayer,
    InventorySnapshot,
};
use crate::inference_manifest::{InferenceManifestLoader, ManifestDiagnostic, ManifestSource};
use crate::model_registry::ModelRegistry;

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
    Agreement,
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
        output.push_str(&format!(
            "\nRoute shadow observations: {}",
            self.route_shadow_observations.len()
        ));
        if let Some(last) = self.route_shadow_observations.last() {
            output.push_str(&format!("\nLast route shadow: {:?}", last.agreement));
        }
        output
    }
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

    pub async fn observe_route_shadow(
        &self,
        grade: crate::routing::CapabilityGradeBand,
        authoritative: Option<&str>,
    ) -> RouteShadowObservation {
        let snapshot = self.store.snapshot().await;
        let mut request = CompatibilityRequest::default();
        if let Some(minimum) = shadow_grade_floor(grade) {
            request.minimum_grades.insert("agentic".into(), minimum);
        }
        let inventory_candidates: Vec<String> = snapshot
            .compatible_offerings(&request)
            .into_iter()
            .filter(|result| result.is_compatible())
            .map(|result| result.offering.id.0.clone())
            .collect();
        let authoritative = authoritative.map(str::to_owned);
        let agreement = match &authoritative {
            None => RouteShadowAgreement::NoAuthoritativeCandidate,
            Some(_) if inventory_candidates.is_empty() => {
                RouteShadowAgreement::NoInventoryCandidate
            }
            Some(route)
                if inventory_candidates
                    .iter()
                    .any(|candidate| candidate == route) =>
            {
                RouteShadowAgreement::Agreement
            }
            Some(_) => RouteShadowAgreement::Divergence,
        };
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
