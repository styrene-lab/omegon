use std::sync::Arc;

use crate::inference_inventory::{InferenceInventoryStore, InventoryLayer, InventorySnapshot};
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
pub struct InferenceRuntimeProjection {
    pub generation: u64,
    pub active_sources: Vec<ManifestSource>,
    pub endpoint_count: usize,
    pub offering_count: usize,
    pub last_rejected_diagnostics: Vec<ManifestDiagnostic>,
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
        output
    }
}

#[derive(Clone, Debug)]
pub struct InferenceRuntimeState {
    store: InferenceInventoryStore,
    loader: InferenceManifestLoader,
    sources: Vec<ManifestSource>,
    active_sources: Arc<tokio::sync::RwLock<Vec<ManifestSource>>>,
    last_rejected_diagnostics: Arc<tokio::sync::RwLock<Vec<ManifestDiagnostic>>>,
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
        }
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
