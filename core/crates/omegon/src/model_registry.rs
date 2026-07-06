//! Centralized model registry loaded from `data/model-registry.json`.
//!
//! This is the single source of truth for model metadata: IDs, context windows,
//! grade mappings, endpoint profiles, and capabilities. Adding a new model
//! means editing the JSON file — zero Rust changes required.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

static REGISTRY_JSON: &str = include_str!("../../../../data/model-registry.json");
static REGISTRY: OnceLock<ModelRegistry> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryFile {
    defaults: HashMap<String, String>,
    #[serde(default)]
    grades: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    endpoints: Vec<ProviderEndpoint>,
    routes: Vec<RouteEntry>,
    models: Vec<ModelEntry>,
    #[serde(default)]
    inference_defaults: InferenceDefaults,
}

/// Defaults for dynamically discovered models not in the registry.
/// These heuristics are intentionally in the JSON so they're easy to
/// adjust via PR without touching Rust code.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceDefaults {
    #[serde(default = "default_context_input")]
    pub context_input: usize,
    #[serde(default = "default_context_output")]
    pub context_output: usize,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub supports_reasoning: bool,
    #[serde(default)]
    pub name_patterns: HashMap<String, Vec<String>>,
}

fn default_context_input() -> usize {
    131_072
}
fn default_context_output() -> usize {
    32_768
}

impl Default for InferenceDefaults {
    fn default() -> Self {
        Self {
            context_input: default_context_input(),
            context_output: default_context_output(),
            capabilities: vec!["instruction".into(), "coding".into()],
            supports_reasoning: false,
            name_patterns: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEndpoint {
    pub id: String,
    pub display_name: String,
    #[serde(rename = "class")]
    pub class_: EndpointClass,
    pub protocol: EndpointProtocol,
    pub base_url: Option<String>,
    pub auth_scheme: EndpointAuthScheme,
    #[serde(default)]
    pub open_ai_compatible_profile: Option<OpenAiCompatibleProfile>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiCompatibleProfile {
    #[serde(default = "default_true")]
    pub supports_chat_completions: bool,
    #[serde(default)]
    pub supports_responses_api: bool,
    #[serde(default = "default_true")]
    pub supports_streaming: bool,
    #[serde(default = "default_true")]
    pub supports_tools: bool,
    #[serde(default)]
    pub unsupported_request_fields: Vec<String>,
    #[serde(default)]
    pub error_profile: OpenAiErrorProfile,
    #[serde(default)]
    pub response_profile: OpenAiResponseProfile,
    #[serde(default)]
    pub required_headers: Vec<String>,
    #[serde(default)]
    pub optional_headers: Vec<String>,
    #[serde(default)]
    pub quirks: Vec<String>,
    #[serde(default)]
    pub docs_url: Option<String>,
    #[serde(default)]
    pub verified_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiErrorProfile {
    #[serde(default = "default_openai_error_type_paths")]
    pub type_paths: Vec<String>,
    #[serde(default = "default_openai_error_message_paths")]
    pub message_paths: Vec<String>,
    #[serde(default = "default_openai_rate_limit_types")]
    pub rate_limit_types: Vec<String>,
}

impl Default for OpenAiErrorProfile {
    fn default() -> Self {
        Self {
            type_paths: default_openai_error_type_paths(),
            message_paths: default_openai_error_message_paths(),
            rate_limit_types: default_openai_rate_limit_types(),
        }
    }
}

fn default_openai_error_type_paths() -> Vec<String> {
    ["error.type", "error.code", "type", "code"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn default_openai_error_message_paths() -> Vec<String> {
    ["error.message", "message", "detail"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn default_openai_rate_limit_types() -> Vec<String> {
    [
        "rate_limit_exceeded",
        "rate_limit_error",
        "rate_limited",
        "too_many_requests",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedEndpointErrorCategory {
    RateLimited,
    Authentication,
    InvalidRequest,
    ProviderUnavailable,
    Unknown,
}

impl NormalizedEndpointErrorCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RateLimited => "rate_limited",
            Self::Authentication => "authentication",
            Self::InvalidRequest => "invalid_request",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEndpointError {
    pub status: u16,
    pub category: NormalizedEndpointErrorCategory,
    pub error_type: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiResponseProfile {
    #[serde(default = "default_openai_tool_call_id_paths")]
    pub tool_call_id_paths: Vec<String>,
    #[serde(default = "default_openai_tool_call_name_paths")]
    pub tool_call_name_paths: Vec<String>,
    #[serde(default = "default_openai_tool_call_arguments_paths")]
    pub tool_call_arguments_paths: Vec<String>,
}

impl Default for OpenAiResponseProfile {
    fn default() -> Self {
        Self {
            tool_call_id_paths: default_openai_tool_call_id_paths(),
            tool_call_name_paths: default_openai_tool_call_name_paths(),
            tool_call_arguments_paths: default_openai_tool_call_arguments_paths(),
        }
    }
}

fn default_openai_tool_call_id_paths() -> Vec<String> {
    ["id"].into_iter().map(str::to_string).collect()
}
fn default_openai_tool_call_name_paths() -> Vec<String> {
    ["function.name"].into_iter().map(str::to_string).collect()
}
fn default_openai_tool_call_arguments_paths() -> Vec<String> {
    ["function.arguments"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments_delta: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EndpointClass {
    LocalDev,
    Upstream,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EndpointProtocol {
    OpenAiCompatible,
    Anthropic,
    GeminiNative,
    OllamaNative,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum EndpointAuthScheme {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "bearerToken")]
    BearerToken {
        #[serde(rename = "secretRef")]
        secret_ref: String,
    },
    #[serde(rename = "apiKeyHeader")]
    ApiKeyHeader {
        header: String,
        #[serde(rename = "secretRef")]
        secret_ref: String,
    },
    #[serde(rename = "oauthProvider")]
    OAuthProvider { provider: String },
    #[serde(rename = "custom")]
    Custom {
        #[serde(rename = "customKind")]
        custom_kind: String,
        #[serde(rename = "secretRef")]
        secret_ref: Option<String>,
    },
}

impl EndpointAuthScheme {
    pub fn required_secret_refs(&self) -> Vec<&str> {
        match self {
            Self::None | Self::OAuthProvider { .. } => vec![],
            Self::BearerToken { secret_ref } | Self::ApiKeyHeader { secret_ref, .. } => {
                vec![secret_ref.as_str()]
            }
            Self::Custom { secret_ref, .. } => secret_ref.as_deref().into_iter().collect(),
        }
    }

    pub fn oauth_provider(&self) -> Option<&str> {
        match self {
            Self::OAuthProvider { provider } => Some(provider.as_str()),
            _ => None,
        }
    }

    pub fn auth_header(&self, secret_value: &str) -> Option<(String, String)> {
        match self {
            Self::BearerToken { .. } => {
                Some(("Authorization".into(), format!("Bearer {secret_value}")))
            }
            Self::ApiKeyHeader { header, .. } => Some((header.clone(), secret_value.to_string())),
            Self::None | Self::OAuthProvider { .. } | Self::Custom { .. } => None,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteEntry {
    pub provider: String,
    #[serde(rename = "modelIdPattern")]
    pub model_id_pattern: String,
    #[serde(rename = "contextCeiling")]
    pub context_ceiling: usize,
    #[serde(default)]
    pub grade: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub provider: String,
    pub name: String,
    /// Stable semantic model identity shared by multiple provider routes.
    ///
    /// A missing value falls back to the provider-native model id so existing
    /// registry entries remain valid during the additive migration.
    #[serde(default, rename = "conceptualModelId")]
    pub conceptual_model_id: Option<String>,
    #[serde(rename = "contextInput")]
    pub context_input: usize,
    #[serde(rename = "contextOutput")]
    pub context_output: usize,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "supportsReasoning")]
    pub supports_reasoning: bool,
    /// Model lineage/creator. Optional so dynamic local/offline routes with
    /// unknown provenance remain valid during discovery.
    #[serde(default)]
    pub producer: Option<String>,
    /// Trust/deployment posture for this route (for example `local`,
    /// `remote-local-network`, `subscription-cloud`, `api-cloud`, or
    /// `broker-cloud`). Kept stringly typed while the policy surface settles.
    #[serde(default, rename = "executionClass")]
    pub execution_class: Option<String>,
}

pub struct ModelRegistry {
    defaults: HashMap<String, String>,
    grades: HashMap<String, HashMap<String, String>>,
    endpoints: Vec<ProviderEndpoint>,
    routes: Vec<RouteEntry>,
    /// Keyed by "provider:model_id"
    models: HashMap<String, ModelEntry>,
    inference_defaults: InferenceDefaults,
}

impl ModelRegistry {
    pub fn global() -> &'static ModelRegistry {
        REGISTRY.get_or_init(|| {
            let file: RegistryFile =
                serde_json::from_str(REGISTRY_JSON).expect("model-registry.json parse error");
            let mut models = HashMap::new();
            for entry in file.models {
                let key = format!("{}:{}", entry.provider, entry.id);
                models.insert(key, entry);
            }
            ModelRegistry {
                defaults: file.defaults,
                grades: file.grades,
                endpoints: validate_endpoints(file.endpoints)
                    .expect("model-registry.json endpoint validation error"),
                routes: file.routes,
                models,
                inference_defaults: file.inference_defaults,
            }
        })
    }

    /// Default model for a provider (bare model ID, not qualified).
    pub fn default_model(&self, provider: &str) -> Option<&str> {
        self.defaults.get(provider).map(|s| s.as_str())
    }

    /// Model for a provider-neutral capability grade + provider (bare model ID).
    pub fn grade_model(&self, grade: &str, provider: &str) -> Option<&str> {
        self.grades
            .get(&grade.to_ascii_uppercase())
            .and_then(|m| m.get(provider))
            .map(|s| s.as_str())
    }

    /// Endpoint definitions from the registry.
    pub fn endpoints(&self) -> &[ProviderEndpoint] {
        &self.endpoints
    }

    /// Endpoint definition by id.
    pub fn endpoint(&self, id: &str) -> Option<&ProviderEndpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.id == id)
    }

    /// Apply OpenAI-compatible endpoint request shaping in-place.
    ///
    /// This removes request fields the selected endpoint profile declares unsupported.
    /// Nested message fields use dotted paths such as `messages[].name`.
    pub fn shape_openai_request(
        &self,
        endpoint_id: &str,
        request: &mut serde_json::Value,
    ) -> Result<(), String> {
        let endpoint = self
            .endpoint(endpoint_id)
            .ok_or_else(|| format!("unknown endpoint '{endpoint_id}'"))?;
        let profile = endpoint
            .open_ai_compatible_profile
            .as_ref()
            .ok_or_else(|| {
                format!("endpoint '{endpoint_id}' is not OpenAI-compatible or lacks a profile")
            })?;
        for field in &profile.unsupported_request_fields {
            remove_request_field(request, field);
        }
        Ok(())
    }

    pub fn normalize_openai_error(
        &self,
        endpoint_id: &str,
        status: reqwest::StatusCode,
        body: &str,
    ) -> Result<NormalizedEndpointError, String> {
        let endpoint = self
            .endpoint(endpoint_id)
            .ok_or_else(|| format!("unknown endpoint '{endpoint_id}'"))?;
        let profile = endpoint
            .open_ai_compatible_profile
            .as_ref()
            .ok_or_else(|| {
                format!("endpoint '{endpoint_id}' is not OpenAI-compatible or lacks a profile")
            })?;
        let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
        let error_type = parsed
            .as_ref()
            .and_then(|value| first_string_path(value, &profile.error_profile.type_paths));
        let message = parsed
            .as_ref()
            .and_then(|value| first_string_path(value, &profile.error_profile.message_paths))
            .unwrap_or_else(|| body.chars().take(200).collect());
        let category = classify_openai_error(
            status.as_u16(),
            error_type.as_deref(),
            &profile.error_profile.rate_limit_types,
        );
        Ok(NormalizedEndpointError {
            status: status.as_u16(),
            category,
            error_type,
            message,
        })
    }

    pub fn normalize_openai_tool_call_deltas(
        &self,
        endpoint_id: &str,
        chunk: &serde_json::Value,
    ) -> Result<Vec<NormalizedToolCallDelta>, String> {
        let endpoint = self
            .endpoint(endpoint_id)
            .ok_or_else(|| format!("unknown endpoint '{endpoint_id}'"))?;
        let profile = endpoint
            .open_ai_compatible_profile
            .as_ref()
            .ok_or_else(|| {
                format!("endpoint '{endpoint_id}' is not OpenAI-compatible or lacks a profile")
            })?;
        let Some(tool_calls) = chunk
            .pointer("/choices/0/delta/tool_calls")
            .and_then(|value| value.as_array())
        else {
            return Ok(vec![]);
        };
        Ok(tool_calls
            .iter()
            .enumerate()
            .map(|(fallback_index, call)| NormalizedToolCallDelta {
                index: call
                    .get("index")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as usize)
                    .unwrap_or(fallback_index),
                id: first_string_path(call, &profile.response_profile.tool_call_id_paths),
                name: first_string_path(call, &profile.response_profile.tool_call_name_paths),
                arguments_delta: first_string_path(
                    call,
                    &profile.response_profile.tool_call_arguments_paths,
                ),
            })
            .collect())
    }

    /// Full model entry by qualified ID ("provider:model_id").
    pub fn model_info(&self, qualified_id: &str) -> Option<&ModelEntry> {
        self.models.get(qualified_id)
    }

    /// Stable semantic model identity for a qualified provider route.
    ///
    /// During the additive migration, registry entries that have not yet been
    /// annotated with `conceptualModelId` use their provider-native model id as
    /// the conceptual fallback.
    pub fn conceptual_model_id(&self, qualified_id: &str) -> Option<&str> {
        let entry = self.model_info(qualified_id)?;
        Some(
            entry
                .conceptual_model_id
                .as_deref()
                .unwrap_or(entry.id.as_str()),
        )
    }

    /// All concrete provider routes that serve the given conceptual model id.
    pub fn routes_for_conceptual_model(&self, conceptual_model_id: &str) -> Vec<&ModelEntry> {
        self.models
            .values()
            .filter(|entry| {
                entry
                    .conceptual_model_id
                    .as_deref()
                    .unwrap_or(entry.id.as_str())
                    == conceptual_model_id
            })
            .collect()
    }

    /// Model producer/lineage for a qualified provider route, when known.
    pub fn producer_for_route(&self, qualified_id: &str) -> Option<&str> {
        self.model_info(qualified_id)?.producer.as_deref()
    }

    /// Execution trust/deployment class for a qualified provider route, when known.
    pub fn execution_class_for_route(&self, qualified_id: &str) -> Option<&str> {
        self.model_info(qualified_id)?.execution_class.as_deref()
    }

    /// All concrete provider routes attributed to a model producer.
    pub fn routes_for_producer(&self, producer: &str) -> Vec<&ModelEntry> {
        self.models
            .values()
            .filter(|entry| entry.producer.as_deref() == Some(producer))
            .collect()
    }

    /// All concrete provider routes with a given execution trust/deployment class.
    pub fn routes_for_execution_class(&self, execution_class: &str) -> Vec<&ModelEntry> {
        self.models
            .values()
            .filter(|entry| entry.execution_class.as_deref() == Some(execution_class))
            .collect()
    }

    /// All model entries for a given provider.
    pub fn models_for_provider(&self, provider: &str) -> Vec<&ModelEntry> {
        self.models
            .values()
            .filter(|m| m.provider == provider)
            .collect()
    }

    /// All model entries.
    pub fn all_models(&self) -> impl Iterator<Item = &ModelEntry> {
        self.models.values()
    }

    /// Context ceiling from route patterns (glob match, highest specificity wins).
    pub fn context_ceiling(&self, provider: &str, model_id: &str) -> Option<usize> {
        let prov = if provider == "ollama" {
            "local"
        } else {
            provider
        };
        let mut best: Option<(usize, usize)> = None; // (specificity, ceiling)
        for route in &self.routes {
            if route.provider != prov && route.provider != provider {
                continue;
            }
            if glob_match(&route.model_id_pattern, model_id) {
                let specificity = route.model_id_pattern.len();
                if best.is_none_or(|(s, _)| specificity > s) {
                    best = Some((specificity, route.context_ceiling));
                }
            }
        }
        best.map(|(_, c)| c)
    }

    /// Exact provider-neutral grade assignment for a provider/model pair.
    pub fn exact_grade(&self, provider: &str, model_id: &str) -> Option<&str> {
        ["S", "A", "B", "C", "D", "F"]
            .into_iter()
            .find(|grade| self.grade_model(grade, provider) == Some(model_id))
    }

    /// Infer provider-neutral capability grade from route patterns.
    pub fn infer_grade(&self, provider: &str, model_id: &str) -> Option<&str> {
        let prov = if provider == "ollama" {
            "local"
        } else {
            provider
        };
        let mut best: Option<(usize, &str)> = None;
        for route in &self.routes {
            if route.provider != prov && route.provider != provider {
                continue;
            }
            if glob_match(&route.model_id_pattern, model_id) {
                let specificity = route.model_id_pattern.len();
                if best.is_none_or(|(s, _)| specificity > s) {
                    let Some(grade) = route_grade(route) else {
                        continue;
                    };
                    best = Some((specificity, grade));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    /// Whether this model supports reasoning/thinking parameters.
    pub fn supports_reasoning(&self, qualified_id: &str) -> bool {
        self.models
            .get(qualified_id)
            .is_some_and(|m| m.supports_reasoning)
    }

    /// Route entries (for settings.rs compatibility during migration).
    pub fn routes(&self) -> &[RouteEntry] {
        &self.routes
    }

    /// Synthesize a ModelEntry for a model not in the registry, using
    /// inference defaults from the JSON config. Capabilities are inferred
    /// from name patterns (e.g. "coder" → coding, "qwq" → reasoning).
    ///
    /// These defaults are intentionally in model-registry.json so they
    /// can be adjusted via PR without Rust changes.
    pub fn infer_unknown_model(&self, provider: &str, model_id: &str) -> ModelEntry {
        let lower = model_id.to_ascii_lowercase();
        let d = &self.inference_defaults;

        // Start with default capabilities
        let mut caps: Vec<String> = d.capabilities.clone();

        // Override from name patterns
        for (cap_name, patterns) in &d.name_patterns {
            if cap_name.starts_with('_') {
                continue; // skip _comment keys
            }
            for pat in patterns {
                if lower.contains(pat) && !caps.contains(cap_name) {
                    caps.push(cap_name.clone());
                    break;
                }
            }
        }

        let supports_reasoning = caps.contains(&"reasoning".to_string());

        ModelEntry {
            id: model_id.to_string(),
            provider: provider.to_string(),
            name: model_id.to_string(),
            conceptual_model_id: None,
            producer: None,
            execution_class: None,
            context_input: d.context_input,
            context_output: d.context_output,
            capabilities: caps,
            description: format!("Dynamically discovered {provider} model"),
            supports_reasoning,
        }
    }

    /// Get model info, falling back to inference for unknown models.
    pub fn model_info_or_infer(&self, provider: &str, model_id: &str) -> ModelEntry {
        let key = format!("{provider}:{model_id}");
        if let Some(entry) = self.models.get(&key) {
            return entry.clone();
        }
        self.infer_unknown_model(provider, model_id)
    }
}

fn first_string_path(value: &serde_json::Value, paths: &[String]) -> Option<String> {
    paths.iter().find_map(|path| string_at_path(value, path))
}

fn string_at_path(value: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_str().map(str::to_string)
}

fn classify_openai_error(
    status: u16,
    error_type: Option<&str>,
    rate_limit_types: &[String],
) -> NormalizedEndpointErrorCategory {
    if status == 429
        || error_type.is_some_and(|kind| {
            rate_limit_types
                .iter()
                .any(|configured| configured.eq_ignore_ascii_case(kind))
        })
    {
        return NormalizedEndpointErrorCategory::RateLimited;
    }
    match status {
        401 | 403 => NormalizedEndpointErrorCategory::Authentication,
        400 | 422 => NormalizedEndpointErrorCategory::InvalidRequest,
        500..=599 => NormalizedEndpointErrorCategory::ProviderUnavailable,
        _ => NormalizedEndpointErrorCategory::Unknown,
    }
}

fn remove_request_field(request: &mut serde_json::Value, field: &str) {
    if let Some(name) = field.strip_prefix("messages[].") {
        if let Some(messages) = request
            .get_mut("messages")
            .and_then(|value| value.as_array_mut())
        {
            for message in messages {
                if let Some(object) = message.as_object_mut() {
                    object.remove(name);
                }
            }
        }
        return;
    }
    if let Some(object) = request.as_object_mut() {
        object.remove(field);
    }
}

fn validate_endpoints(endpoints: Vec<ProviderEndpoint>) -> Result<Vec<ProviderEndpoint>, String> {
    const RESERVED: &[&str] = &["auto", "local", "upstream"];
    let mut seen = std::collections::HashSet::new();
    for endpoint in &endpoints {
        if RESERVED.contains(&endpoint.id.as_str()) {
            return Err(format!(
                "endpoint id '{}' is reserved for provider selection",
                endpoint.id
            ));
        }
        if !seen.insert(endpoint.id.as_str()) {
            return Err(format!("duplicate endpoint id '{}'", endpoint.id));
        }
        match endpoint.protocol {
            EndpointProtocol::OpenAiCompatible if endpoint.open_ai_compatible_profile.is_none() => {
                return Err(format!(
                    "OpenAI-compatible endpoint '{}' lacks an openAiCompatibleProfile",
                    endpoint.id
                ));
            }
            EndpointProtocol::Anthropic
            | EndpointProtocol::GeminiNative
            | EndpointProtocol::OllamaNative => {
                if endpoint.open_ai_compatible_profile.is_some() {
                    return Err(format!(
                        "non-OpenAI-compatible endpoint '{}' must not declare openAiCompatibleProfile",
                        endpoint.id
                    ));
                }
            }
            EndpointProtocol::OpenAiCompatible => {}
        }
    }
    Ok(endpoints)
}

fn route_grade(route: &RouteEntry) -> Option<&str> {
    route.grade.as_deref()
}

/// Simple glob matching: `*` at end matches any suffix, otherwise exact.
fn glob_match(pattern: &str, value: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        value == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_loads_without_panic() {
        let reg = ModelRegistry::global();
        assert!(!reg.defaults.is_empty());
        assert!(!reg.grades.is_empty());
        assert!(!reg.models.is_empty());
        assert!(!reg.routes.is_empty());
    }

    #[test]
    fn default_model_lookup() {
        let reg = ModelRegistry::global();
        assert_eq!(reg.default_model("openai"), Some("gpt-5.5"));
        assert_eq!(reg.default_model("openai-codex"), Some("gpt-5.5"));
        assert_eq!(reg.default_model("github-copilot"), Some("gpt-5.5"));
        assert_eq!(reg.default_model("anthropic"), Some("claude-fable-5"));
        assert_eq!(reg.default_model("nonexistent"), None);
    }

    #[test]
    fn grade_model_lookup() {
        let reg = ModelRegistry::global();
        assert_eq!(reg.grade_model("S", "openai"), Some("gpt-5.5"));
        assert_eq!(reg.grade_model("S", "github-copilot"), Some("gpt-5.5"));
        assert_eq!(
            reg.grade_model("B", "github-copilot"),
            Some("claude-sonnet-4.6")
        );
        assert_eq!(reg.grade_model("S", "anthropic"), Some("claude-fable-5"));
        assert_eq!(
            reg.grade_model("D", "anthropic"),
            Some("claude-haiku-4-5-20251001")
        );
        assert_eq!(reg.grade_model("S", "nonexistent"), None);
    }

    #[test]
    fn claude_opus_4_8_is_registered_as_frontier_anthropic_model() {
        let reg = ModelRegistry::global();
        let info = reg.model_info("anthropic:claude-opus-4-8").unwrap();
        assert_eq!(info.name, "Claude Opus 4.8");
        assert_eq!(info.context_input, 1_000_000);
        assert_eq!(info.context_output, 131_072);
        assert_eq!(reg.infer_grade("anthropic", "claude-opus-4-8"), Some("S"));
    }

    #[test]
    fn model_info_lookup() {
        let reg = ModelRegistry::global();
        let info = reg.model_info("openai:gpt-5.5").unwrap();
        assert_eq!(info.name, "GPT-5.5");
        assert_eq!(info.context_input, 1_000_000);
        assert!(info.supports_reasoning);
    }

    #[test]
    fn conceptual_model_lookup_groups_equivalent_provider_routes() {
        let reg = ModelRegistry::global();

        assert_eq!(
            reg.conceptual_model_id("anthropic:claude-sonnet-4-6"),
            Some("claude-sonnet-4.6")
        );
        assert_eq!(
            reg.conceptual_model_id("github-copilot:claude-sonnet-4.6"),
            Some("claude-sonnet-4.6")
        );
        assert_eq!(
            reg.conceptual_model_id("github-copilot:claude-opus-4.7"),
            Some("claude-opus-4.7")
        );

        let mut routes: Vec<String> = reg
            .routes_for_conceptual_model("claude-sonnet-4.6")
            .into_iter()
            .map(|entry| format!("{}:{}", entry.provider, entry.id))
            .collect();
        routes.sort();

        assert!(routes.contains(&"anthropic:claude-sonnet-4-6".to_string()));
        assert!(routes.contains(&"github-copilot:claude-sonnet-4.6".to_string()));
        assert!(routes.contains(&"perplexity:anthropic/claude-sonnet-4-6".to_string()));
    }

    #[test]
    fn producer_and_execution_class_are_distinct_from_provider_route() {
        let reg = ModelRegistry::global();

        assert_eq!(
            reg.producer_for_route("github-copilot:claude-sonnet-4.6"),
            Some("anthropic")
        );
        assert_eq!(
            reg.execution_class_for_route("github-copilot:claude-sonnet-4.6"),
            Some("subscription-cloud")
        );
        assert_eq!(
            reg.producer_for_route("github-copilot:gpt-5.5"),
            Some("openai")
        );
        assert_eq!(
            reg.producer_for_route("anthropic:claude-sonnet-4-6"),
            Some("anthropic")
        );

        let anthropic_routes: Vec<String> = reg
            .routes_for_producer("anthropic")
            .into_iter()
            .map(|entry| format!("{}:{}", entry.provider, entry.id))
            .collect();
        assert!(anthropic_routes.contains(&"github-copilot:claude-sonnet-4.6".to_string()));
        assert!(anthropic_routes.contains(&"anthropic:claude-sonnet-4-6".to_string()));
    }

    #[test]
    fn dynamic_local_models_remain_valid_without_producer_metadata() {
        let reg = ModelRegistry::global();
        let model = reg.infer_unknown_model("ollama", "my-finetune:latest");

        assert_eq!(model.provider, "ollama");
        assert_eq!(model.producer, None);
        assert_eq!(model.execution_class, None);
        assert_eq!(model.conceptual_model_id, None);
        assert_eq!(
            reg.model_info_or_infer("ollama", "my-finetune:latest")
                .producer,
            None
        );
    }

    #[test]
    fn conceptual_model_lookup_falls_back_to_provider_model_id() {
        let reg = ModelRegistry::global();
        assert_eq!(
            reg.conceptual_model_id("openrouter:qwen/qwen-qwq-32b"),
            Some("qwen/qwen-qwq-32b")
        );
    }

    #[test]
    fn context_ceiling_glob() {
        let reg = ModelRegistry::global();
        assert_eq!(
            reg.context_ceiling("anthropic", "claude-opus-4-6"),
            Some(1_000_000)
        );
        assert_eq!(reg.context_ceiling("openai", "gpt-5.5"), Some(1_000_000));
        assert_eq!(reg.context_ceiling("openai", "gpt-5.4"), Some(1_000_000));
        assert_eq!(reg.context_ceiling("openai", "gpt-5.4-mini"), Some(400_000));
    }

    #[test]
    fn infer_grade_from_routes() {
        let reg = ModelRegistry::global();
        assert_eq!(reg.infer_grade("openai", "gpt-5.5"), Some("S"));
        assert_eq!(reg.exact_grade("openai-codex", "gpt-5.5"), Some("S"));
        assert_eq!(
            reg.infer_grade("anthropic", "claude-haiku-4-5-20251001"),
            Some("D")
        );
        assert_eq!(reg.infer_grade("openai", "gpt-5-mini"), Some("D"));
    }

    #[test]
    fn exact_model_entries_agree_with_matching_routes() {
        let reg = ModelRegistry::global();
        for model in reg.all_models() {
            if let Some(route_ceiling) = reg.context_ceiling(&model.provider, &model.id) {
                assert_eq!(
                    route_ceiling, model.context_input,
                    "route/model context mismatch for {}:{}",
                    model.provider, model.id
                );
            }
        }
    }

    #[test]
    fn defaults_and_grade_models_have_context_constraints() {
        let reg = ModelRegistry::global();
        for (provider, model_id) in &reg.defaults {
            let info = reg.model_info_or_infer(provider, model_id);
            assert!(
                info.context_input > 0,
                "default model lacks context constraint: {provider}:{model_id}"
            );
        }
        for (grade, providers) in &reg.grades {
            for (provider, model_id) in providers {
                let info = reg.model_info_or_infer(provider, model_id);
                assert!(
                    info.context_input > 0,
                    "grade model lacks context constraint: {grade} {provider}:{model_id}"
                );
            }
        }
    }
    #[test]
    fn infer_unknown_model_applies_name_patterns() {
        let reg = ModelRegistry::global();
        let coder = reg.infer_unknown_model("ollama-cloud", "qwen3-coder:480b-cloud");
        assert!(coder.capabilities.contains(&"coding".to_string()));

        let reasoner = reg.infer_unknown_model("ollama-cloud", "deepseek-r1:70b");
        assert!(reasoner.capabilities.contains(&"reasoning".to_string()));
        assert!(reasoner.supports_reasoning);

        let generic = reg.infer_unknown_model("ollama-cloud", "some-model:7b");
        assert!(!generic.supports_reasoning);
        assert_eq!(generic.context_input, 131_072);
    }

    #[test]
    fn model_info_or_infer_falls_back() {
        let reg = ModelRegistry::global();
        // Known model returns real data
        let known = reg.model_info_or_infer("openai", "gpt-5.5");
        assert_eq!(known.name, "GPT-5.5");
        // Unknown model returns inferred data
        let unknown = reg.model_info_or_infer("ollama-cloud", "mystery-model:13b");
        assert_eq!(unknown.provider, "ollama-cloud");
        assert_eq!(unknown.context_input, 131_072);
    }

    #[test]
    fn openai_request_shaping_removes_unsupported_fields() {
        let reg = ModelRegistry::global();
        let mut request = serde_json::json!({
            "model": "llama-3.3-70b-versatile",
            "logprobs": true,
            "top_logprobs": 2,
            "messages": [
                {"role": "user", "name": "operator", "content": "hello"}
            ]
        });

        reg.shape_openai_request("groq", &mut request).unwrap();

        assert!(request.get("logprobs").is_none());
        assert!(request.get("top_logprobs").is_none());
        assert!(request["messages"][0].get("name").is_none());
        assert_eq!(request["messages"][0]["content"], "hello");
    }

    #[test]
    fn request_shaping_rejects_non_openai_endpoint() {
        let reg = ModelRegistry::global();
        let mut request = serde_json::json!({"messages": []});
        assert!(reg.shape_openai_request("anthropic", &mut request).is_err());
    }

    #[test]
    fn glob_match_works() {
        assert!(glob_match("gpt-5.5*", "gpt-5.5"));
        assert!(glob_match("gpt-5.5*", "gpt-5.5-turbo"));
        assert!(!glob_match("gpt-5.5*", "gpt-5.4"));
        assert!(glob_match("gpt-5.4", "gpt-5.4"));
        assert!(!glob_match("gpt-5.4", "gpt-5.4-mini"));
    }
    #[test]
    fn endpoint_auth_scheme_exposes_secret_refs_and_headers() {
        let reg = ModelRegistry::global();
        let groq = reg.endpoint("groq").expect("groq endpoint");
        assert_eq!(
            groq.auth_scheme.required_secret_refs(),
            vec!["GROQ_API_KEY"]
        );
        assert_eq!(
            groq.auth_scheme.auth_header("secret"),
            Some(("Authorization".into(), "Bearer secret".into()))
        );

        let anthropic = reg.endpoint("anthropic").expect("anthropic endpoint");
        assert_eq!(
            anthropic.auth_scheme.required_secret_refs(),
            Vec::<&str>::new()
        );
        assert_eq!(anthropic.auth_scheme.oauth_provider(), Some("anthropic"));
        assert_eq!(anthropic.auth_scheme.auth_header("secret"), None);

        let ollama = reg.endpoint("ollama").expect("ollama endpoint");
        assert_eq!(
            ollama.auth_scheme.required_secret_refs(),
            Vec::<&str>::new()
        );
        assert_eq!(ollama.auth_scheme.auth_header("secret"), None);
    }
    #[test]
    fn every_openai_compatible_endpoint_has_verified_profile() {
        let reg = ModelRegistry::global();
        for endpoint in reg.endpoints() {
            if endpoint.protocol != EndpointProtocol::OpenAiCompatible {
                continue;
            }
            let profile = endpoint
                .open_ai_compatible_profile
                .as_ref()
                .unwrap_or_else(|| {
                    panic!(
                        "OpenAI-compatible endpoint {} is missing openAiCompatibleProfile",
                        endpoint.id
                    )
                });
            assert!(
                profile.supports_chat_completions
                    || profile.supports_responses_api
                    || profile.supports_streaming
                    || profile.supports_tools,
                "OpenAI-compatible endpoint {} declares no usable capability",
                endpoint.id
            );
            assert!(
                profile
                    .docs_url
                    .as_deref()
                    .is_some_and(|url| url.starts_with("https://")),
                "OpenAI-compatible endpoint {} must carry an https docsUrl",
                endpoint.id
            );
            assert!(
                profile.verified_at.as_deref().is_some_and(|date| {
                    let bytes = date.as_bytes();
                    bytes.len() == 10
                        && bytes[4] == b'-'
                        && bytes[7] == b'-'
                        && bytes
                            .iter()
                            .enumerate()
                            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
                }),
                "OpenAI-compatible endpoint {} must carry verifiedAt as YYYY-MM-DD",
                endpoint.id
            );
        }
    }
    #[test]
    fn openai_error_normalization_maps_rate_limit_type() {
        let reg = ModelRegistry::global();
        let err = reg
            .normalize_openai_error(
                "groq",
                reqwest::StatusCode::BAD_REQUEST,
                r#"{"error":{"type":"rate_limit_exceeded","message":"slow down"}}"#,
            )
            .unwrap();
        assert_eq!(err.category, NormalizedEndpointErrorCategory::RateLimited);
        assert_eq!(err.error_type.as_deref(), Some("rate_limit_exceeded"));
        assert_eq!(err.message, "slow down");
    }

    #[test]
    fn openai_error_normalization_maps_auth_status() {
        let reg = ModelRegistry::global();
        let err = reg
            .normalize_openai_error(
                "openrouter",
                reqwest::StatusCode::UNAUTHORIZED,
                r#"{"message":"bad key","code":"invalid_api_key"}"#,
            )
            .unwrap();
        assert_eq!(
            err.category,
            NormalizedEndpointErrorCategory::Authentication
        );
        assert_eq!(err.error_type.as_deref(), Some("invalid_api_key"));
        assert_eq!(err.message, "bad key");
    }

    #[test]
    fn openai_error_normalization_rejects_non_openai_endpoint() {
        let reg = ModelRegistry::global();
        assert!(reg
            .normalize_openai_error("anthropic", reqwest::StatusCode::TOO_MANY_REQUESTS, "{}")
            .is_err());
    }
    #[test]
    fn openai_tool_call_delta_normalization_uses_profile_paths() {
        let reg = ModelRegistry::global();
        let chunk = serde_json::json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0,
                "id": "call_1",
                "function": {"name": "bash", "arguments": "{\"command\":"}
            }]}}]
        });
        let deltas = reg
            .normalize_openai_tool_call_deltas("openai", &chunk)
            .unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].index, 0);
        assert_eq!(deltas[0].id.as_deref(), Some("call_1"));
        assert_eq!(deltas[0].name.as_deref(), Some("bash"));
        assert_eq!(deltas[0].arguments_delta.as_deref(), Some("{\"command\":"));
    }

    #[test]
    fn openai_tool_call_delta_normalization_rejects_non_openai_endpoint() {
        let reg = ModelRegistry::global();
        assert!(reg
            .normalize_openai_tool_call_deltas("anthropic", &serde_json::json!({}))
            .is_err());
    }
}
