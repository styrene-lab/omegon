use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::inference_inventory::{
    AdapterId, CapabilityGrade, ConceptualModelId, ConceptualModelPatch, EndpointGroupId,
    EndpointGroupPatch, EndpointId, EndpointPatch, EvidenceKind, ExtensionMetadata,
    InferenceInventoryStore, InventoryLayer, InventorySource, Modality, OfferingId, OfferingPatch,
    PolicyAttributes, TransportSpec,
};

const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct ManifestSource {
    pub source: InventorySource,
    pub path: PathBuf,
    pub required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestPhase {
    Read,
    Parse,
    Schema,
    Conversion,
    Validation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestDiagnostic {
    pub source: InventorySource,
    pub path: PathBuf,
    pub phase: ManifestPhase,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    schema_version: u32,
    #[serde(default)]
    groups: Vec<GroupRecord>,
    #[serde(default)]
    conceptual_models: Vec<ConceptualModelRecord>,
    #[serde(default)]
    endpoints: Vec<EndpointRecord>,
    #[serde(default)]
    offerings: Vec<OfferingRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupRecord {
    id: String,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConceptualModelRecord {
    id: String,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointRecord {
    id: String,
    group: Option<String>,
    clear_group: Option<bool>,
    adapter: Option<String>,
    transport: Option<TransportRecord>,
    secret_refs: Option<Vec<String>>,
    policy: Option<PolicyAttributes>,
    extensions: Option<ExtensionMetadata>,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum TransportRecord {
    Http { base_url: String },
    LocalProcess { command_ref: String },
    UnixSocket { path: String },
    Managed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OfferingRecord {
    id: String,
    endpoint: Option<String>,
    native_model_id: Option<String>,
    display_name: Option<String>,
    conceptual_model: Option<String>,
    clear_conceptual_model: Option<bool>,
    input_modalities: Option<BTreeSet<String>>,
    output_modalities: Option<BTreeSet<String>>,
    #[serde(default)]
    capabilities: BTreeMap<String, bool>,
    #[serde(default)]
    capability_grades: BTreeMap<String, String>,
    context_input: Option<usize>,
    clear_context_input: Option<bool>,
    context_output: Option<usize>,
    clear_context_output: Option<bool>,
    extensions: Option<ExtensionMetadata>,
    enabled: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct InferenceManifestLoader {
    embedded: InventoryLayer,
    sources: Vec<ManifestSource>,
}

impl InferenceManifestLoader {
    pub fn new(embedded: InventoryLayer, sources: Vec<ManifestSource>) -> Self {
        Self { embedded, sources }
    }

    pub fn default_sources(home: &Path, project_root: &Path) -> Vec<ManifestSource> {
        vec![
            ManifestSource {
                source: InventorySource::User,
                path: home.join("inference.toml"),
                required: false,
            },
            ManifestSource {
                source: InventorySource::Project,
                path: project_root.join(".omegon/inference.toml"),
                required: false,
            },
        ]
    }

    pub fn load_layers(&self) -> Result<Vec<InventoryLayer>, Vec<ManifestDiagnostic>> {
        let mut layers = vec![self.embedded.clone()];
        let mut diagnostics = Vec::new();
        for source in &self.sources {
            match load_source(source) {
                Ok(Some(layer)) => layers.push(layer),
                Ok(None) => {}
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
        if diagnostics.is_empty() {
            Ok(layers)
        } else {
            Err(diagnostics)
        }
    }

    pub async fn reload(
        &self,
        store: &InferenceInventoryStore,
    ) -> Result<u64, Vec<ManifestDiagnostic>> {
        let layers = self.load_layers()?;
        match store.refresh(layers).await {
            Ok(snapshot) => Ok(snapshot.generation),
            Err(errors) => Err(vec![ManifestDiagnostic {
                source: InventorySource::Session,
                path: PathBuf::new(),
                phase: ManifestPhase::Validation,
                message: errors.join("; "),
            }]),
        }
    }
}

fn load_source(source: &ManifestSource) -> Result<Option<InventoryLayer>, ManifestDiagnostic> {
    let content = match std::fs::read_to_string(&source.path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !source.required => {
            return Ok(None);
        }
        Err(error) => {
            return Err(diagnostic(
                source,
                ManifestPhase::Read,
                format!("manifest could not be read ({:?})", error.kind()),
            ));
        }
    };
    let file: ManifestFile = toml::from_str(&content).map_err(|error| {
        let location = error
            .span()
            .map(|span| format!(" near byte {}", span.start))
            .unwrap_or_default();
        diagnostic(
            source,
            ManifestPhase::Parse,
            format!("invalid TOML{location}"),
        )
    })?;
    if file.schema_version != SCHEMA_VERSION {
        return Err(diagnostic(
            source,
            ManifestPhase::Schema,
            format!("unsupported schema version {}", file.schema_version),
        ));
    }
    convert(source, file).map(Some)
}

fn diagnostic(
    source: &ManifestSource,
    phase: ManifestPhase,
    message: String,
) -> ManifestDiagnostic {
    ManifestDiagnostic {
        source: source.source,
        path: source.path.clone(),
        phase,
        message,
    }
}

fn convert(
    source: &ManifestSource,
    file: ManifestFile,
) -> Result<InventoryLayer, ManifestDiagnostic> {
    let mut layer = InventoryLayer::new(source.source, EvidenceKind::Declared);
    for record in file.groups {
        layer.providers.insert(
            EndpointGroupId(record.id),
            EndpointGroupPatch {
                display_name: record.display_name,
            },
        );
    }
    for record in file.conceptual_models {
        layer.conceptual_models.insert(
            ConceptualModelId(record.id),
            ConceptualModelPatch {
                display_name: record.display_name,
            },
        );
    }
    for record in file.endpoints {
        if record.group.is_some() && record.clear_group.unwrap_or(false) {
            return Err(diagnostic(
                source,
                ManifestPhase::Conversion,
                format!("endpoint '{}' both sets and clears group", record.id),
            ));
        }
        let transport = match record.transport {
            Some(TransportRecord::Http { base_url }) => Some(TransportSpec::Http { base_url }),
            Some(TransportRecord::LocalProcess { command_ref }) => {
                Some(TransportSpec::LocalProcess { command_ref })
            }
            Some(TransportRecord::UnixSocket { path }) => Some(TransportSpec::UnixSocket { path }),
            Some(TransportRecord::Managed) => {
                return Err(diagnostic(
                    source,
                    ManifestPhase::Conversion,
                    format!("endpoint '{}' uses reserved managed transport", record.id),
                ));
            }
            None => None,
        };
        let group = if record.clear_group.unwrap_or(false) {
            Some(None)
        } else {
            record.group.map(|id| Some(EndpointGroupId(id)))
        };
        layer.endpoints.insert(
            EndpointId(record.id),
            EndpointPatch {
                group,
                adapter: record.adapter.map(AdapterId),
                transport,
                secret_refs: record.secret_refs,
                policy_attributes: record.policy,
                extensions: record.extensions,
                enabled: record.enabled,
            },
        );
    }
    for record in file.offerings {
        if record.conceptual_model.is_some() && record.clear_conceptual_model.unwrap_or(false) {
            return Err(diagnostic(
                source,
                ManifestPhase::Conversion,
                format!(
                    "offering '{}' both sets and clears conceptual model",
                    record.id
                ),
            ));
        }
        let conceptual_model = if record.clear_conceptual_model.unwrap_or(false) {
            Some(None)
        } else {
            record
                .conceptual_model
                .map(|id| Some(ConceptualModelId(id)))
        };
        let capability_grades = record
            .capability_grades
            .into_iter()
            .map(|(capability, grade)| {
                parse_grade(&grade)
                    .map(|grade| (capability, grade))
                    .ok_or_else(|| {
                        diagnostic(
                            source,
                            ManifestPhase::Conversion,
                            format!("offering '{}' has invalid capability grade", record.id),
                        )
                    })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        layer.offerings.insert(
            OfferingId(record.id),
            OfferingPatch {
                endpoint: record.endpoint.map(EndpointId),
                native_model_id: record.native_model_id,
                display_name: record.display_name,
                conceptual_model,
                input_modalities: record
                    .input_modalities
                    .map(|items| items.into_iter().map(Modality).collect()),
                output_modalities: record
                    .output_modalities
                    .map(|items| items.into_iter().map(Modality).collect()),
                capabilities: record.capabilities,
                capability_grades,
                context_input: optional_clear(record.context_input, record.clear_context_input),
                context_output: optional_clear(record.context_output, record.clear_context_output),
                extensions: record.extensions,
                enabled: record.enabled,
            },
        );
    }
    Ok(layer)
}

fn optional_clear<T>(value: Option<T>, clear: Option<bool>) -> Option<Option<T>> {
    if clear.unwrap_or(false) {
        Some(None)
    } else {
        value.map(Some)
    }
}

fn parse_grade(value: &str) -> Option<CapabilityGrade> {
    match value {
        "F" => Some(CapabilityGrade::F),
        "D" => Some(CapabilityGrade::D),
        "C" => Some(CapabilityGrade::C),
        "B" => Some(CapabilityGrade::B),
        "A" => Some(CapabilityGrade::A),
        "S" => Some(CapabilityGrade::S),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::inference_inventory::InventorySnapshot;
    use crate::model_registry::ModelRegistry;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "omegon-inference-manifest-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn bootstrap() -> InventoryLayer {
        InventoryLayer::embedded_registry(ModelRegistry::global())
    }

    #[tokio::test]
    async fn project_manifest_adds_standalone_endpoint() {
        let dir = temp_dir();
        let path = dir.join("inference.toml");
        std::fs::write(
            &path,
            r#"schema_version = 1
[[endpoints]]
id = "private-chat"
adapter = "chat-completions"
secret_refs = ["PRIVATE_CHAT_TOKEN"]
[endpoints.transport]
kind = "http"
base_url = "https://inference.internal/v1"
[[offerings]]
id = "private-chat:model-a"
endpoint = "private-chat"
native_model_id = "model-a"
input_modalities = ["text"]
output_modalities = ["text"]
"#,
        )
        .unwrap();
        let loader = InferenceManifestLoader::new(
            bootstrap(),
            vec![ManifestSource {
                source: InventorySource::Project,
                path,
                required: true,
            }],
        );
        let store = InferenceInventoryStore::new(InventorySnapshot::empty());
        assert_eq!(loader.reload(&store).await.unwrap(), 1);
        let snapshot = store.snapshot().await;
        assert!(
            snapshot
                .endpoints
                .contains_key(&EndpointId("private-chat".into()))
        );
        assert!(
            snapshot
                .offerings
                .contains_key(&OfferingId("private-chat:model-a".into()))
        );
    }

    #[tokio::test]
    async fn failed_reload_retains_active_snapshot() {
        let store =
            InferenceInventoryStore::new(InventorySnapshot::build(4, vec![bootstrap()]).unwrap());
        let dir = temp_dir();
        let path = dir.join("bad.toml");
        std::fs::write(
            &path,
            "schema_version = 1\n[[endpoints]]\nid = 'bad'\nadapter = 'chat-completions'\nsecret_refs = ['token-value']\n",
        )
        .unwrap();
        let loader = InferenceManifestLoader::new(
            bootstrap(),
            vec![ManifestSource {
                source: InventorySource::Project,
                path,
                required: true,
            }],
        );
        let before = store.snapshot().await;
        let diagnostics = loader.reload(&store).await.unwrap_err();
        let after = store.snapshot().await;
        assert_eq!(before.generation, after.generation);
        assert_eq!(before.offerings, after.offerings);
        assert!(!diagnostics[0].message.contains("token-value"));
    }

    #[test]
    fn optional_missing_is_ignored_and_required_missing_is_diagnostic() {
        let dir = temp_dir();
        let missing = dir.join("missing.toml");
        let optional = InferenceManifestLoader::new(
            bootstrap(),
            vec![ManifestSource {
                source: InventorySource::User,
                path: missing.clone(),
                required: false,
            }],
        );
        assert_eq!(optional.load_layers().unwrap().len(), 1);
        let required = InferenceManifestLoader::new(
            bootstrap(),
            vec![ManifestSource {
                source: InventorySource::Project,
                path: missing,
                required: true,
            }],
        );
        assert_eq!(
            required.load_layers().unwrap_err()[0].phase,
            ManifestPhase::Read
        );
    }

    #[test]
    fn unknown_version_and_managed_transport_are_rejected() {
        let dir = temp_dir();
        let version = dir.join("version.toml");
        std::fs::write(&version, "schema_version = 99\n").unwrap();
        let source = ManifestSource {
            source: InventorySource::Project,
            path: version,
            required: true,
        };
        assert_eq!(
            load_source(&source).unwrap_err().phase,
            ManifestPhase::Schema
        );

        let managed = dir.join("managed.toml");
        std::fs::write(
            &managed,
            "schema_version = 1\n[[endpoints]]\nid='x'\nadapter='chat-completions'\n[endpoints.transport]\nkind='managed'\n",
        )
        .unwrap();
        let source = ManifestSource {
            path: managed,
            ..source
        };
        assert_eq!(
            load_source(&source).unwrap_err().phase,
            ManifestPhase::Conversion
        );
    }

    #[test]
    fn malformed_toml_diagnostic_does_not_echo_content() {
        let dir = temp_dir();
        let path = dir.join("malformed.toml");
        std::fs::write(&path, "schema_version = 'SECRET-VALUE' [").unwrap();
        let source = ManifestSource {
            source: InventorySource::Project,
            path,
            required: true,
        };
        let diagnostic = load_source(&source).unwrap_err();
        assert_eq!(diagnostic.phase, ManifestPhase::Parse);
        assert!(!diagnostic.message.contains("SECRET-VALUE"));
    }
}
