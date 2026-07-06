use crate::model_registry::{ModelEntry, ModelRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderPolicy {
    Auto,
    CopilotOnly,
    DirectFirst,
    BrokerFirst,
    LocalOnly,
}

impl ProviderPolicy {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "auto" => Some(Self::Auto),
            "copilot-only" | "github-copilot" => Some(Self::CopilotOnly),
            "direct-first" => Some(Self::DirectFirst),
            "broker-first" => Some(Self::BrokerFirst),
            "local-only" | "local" => Some(Self::LocalOnly),
            _ => None,
        }
    }

    fn allows_provider(self, provider: &str) -> bool {
        match self {
            Self::Auto | Self::DirectFirst | Self::BrokerFirst => true,
            Self::CopilotOnly => provider == "github-copilot",
            Self::LocalOnly => matches!(provider, "ollama" | "local"),
        }
    }

    fn provider_rank(self, provider: &str) -> usize {
        match self {
            Self::CopilotOnly => usize::from(provider != "github-copilot"),
            Self::LocalOnly => usize::from(!matches!(provider, "ollama" | "local")),
            Self::DirectFirst => match provider_class(provider) {
                ProviderClass::Direct => 0,
                ProviderClass::Subscription => 1,
                ProviderClass::Broker => 2,
                ProviderClass::Local => 3,
                ProviderClass::Other => 4,
            },
            Self::BrokerFirst => match provider_class(provider) {
                ProviderClass::Broker => 0,
                ProviderClass::Subscription => 1,
                ProviderClass::Direct => 2,
                ProviderClass::Local => 3,
                ProviderClass::Other => 4,
            },
            Self::Auto => match provider_class(provider) {
                ProviderClass::Subscription => 0,
                ProviderClass::Direct => 1,
                ProviderClass::Broker => 2,
                ProviderClass::Local => 3,
                ProviderClass::Other => 4,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderClass {
    Subscription,
    Direct,
    Broker,
    Local,
    Other,
}

fn provider_class(provider: &str) -> ProviderClass {
    match provider {
        "github-copilot" => ProviderClass::Subscription,
        "anthropic" | "openai" | "openai-codex" | "google" | "google-antigravity" | "xai"
        | "mistral" | "groq" | "cerebras" | "ollama-cloud" | "opencode-go" => ProviderClass::Direct,
        "openrouter" | "perplexity" | "huggingface" => ProviderClass::Broker,
        "ollama" | "local" => ProviderClass::Local,
        _ => ProviderClass::Other,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSemanticRoute {
    pub provider: String,
    pub provider_model_id: String,
    pub qualified_model: String,
    pub conceptual_model_id: String,
    pub producer: Option<String>,
    pub execution_class: Option<String>,
    pub exact_route_pin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticRouteResolutionError {
    UnknownRoute {
        requested: String,
    },
    UnknownConceptualModel {
        requested: String,
    },
    ProviderNotAllowed {
        requested: String,
        provider: String,
        policy: ProviderPolicy,
    },
    NoPolicyCompliantRoute {
        requested: String,
        policy: ProviderPolicy,
    },
}

pub fn resolve_semantic_model_route(
    registry: &ModelRegistry,
    requested: &str,
    policy: ProviderPolicy,
) -> Result<ResolvedSemanticRoute, SemanticRouteResolutionError> {
    let requested = requested.trim();
    if let Some((provider, _model_id)) = requested.split_once(':') {
        let entry = registry.model_info(requested).ok_or_else(|| {
            SemanticRouteResolutionError::UnknownRoute {
                requested: requested.to_string(),
            }
        })?;
        if !policy.allows_provider(provider) {
            return Err(SemanticRouteResolutionError::ProviderNotAllowed {
                requested: requested.to_string(),
                provider: provider.to_string(),
                policy,
            });
        }
        return Ok(resolved_from_entry(registry, entry, true));
    }

    let mut routes = registry.routes_for_conceptual_model(requested);
    if routes.is_empty() {
        return Err(SemanticRouteResolutionError::UnknownConceptualModel {
            requested: requested.to_string(),
        });
    }
    routes.retain(|entry| policy.allows_provider(&entry.provider));
    routes.sort_by(|left, right| {
        policy
            .provider_rank(&left.provider)
            .cmp(&policy.provider_rank(&right.provider))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.id.cmp(&right.id))
    });
    let entry = routes.into_iter().next().ok_or_else(|| {
        SemanticRouteResolutionError::NoPolicyCompliantRoute {
            requested: requested.to_string(),
            policy,
        }
    })?;
    Ok(resolved_from_entry(registry, entry, false))
}

fn resolved_from_entry(
    registry: &ModelRegistry,
    entry: &ModelEntry,
    exact_route_pin: bool,
) -> ResolvedSemanticRoute {
    let qualified_model = format!("{}:{}", entry.provider, entry.id);
    ResolvedSemanticRoute {
        provider: entry.provider.clone(),
        provider_model_id: entry.id.clone(),
        qualified_model: qualified_model.clone(),
        conceptual_model_id: registry
            .conceptual_model_id(&qualified_model)
            .unwrap_or(entry.id.as_str())
            .to_string(),
        producer: entry.producer.clone(),
        execution_class: entry.execution_class.clone(),
        exact_route_pin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copilot_only_resolves_conceptual_sonnet_to_copilot_route() {
        let route = resolve_semantic_model_route(
            ModelRegistry::global(),
            "claude-sonnet-4.6",
            ProviderPolicy::CopilotOnly,
        )
        .unwrap();

        assert_eq!(route.qualified_model, "github-copilot:claude-sonnet-4.6");
        assert_eq!(route.conceptual_model_id, "claude-sonnet-4.6");
        assert_eq!(route.producer.as_deref(), Some("anthropic"));
        assert_eq!(route.execution_class.as_deref(), Some("subscription-cloud"));
        assert!(!route.exact_route_pin);
    }

    #[test]
    fn copilot_only_resolves_conceptual_opus_to_copilot_route() {
        let route = resolve_semantic_model_route(
            ModelRegistry::global(),
            "claude-opus-4.7",
            ProviderPolicy::CopilotOnly,
        )
        .unwrap();

        assert_eq!(route.qualified_model, "github-copilot:claude-opus-4.7");
        assert_eq!(route.conceptual_model_id, "claude-opus-4.7");
        assert_eq!(route.producer.as_deref(), Some("anthropic"));
    }

    #[test]
    fn copilot_only_resolves_conceptual_gpt_to_copilot_route() {
        let route = resolve_semantic_model_route(
            ModelRegistry::global(),
            "gpt-5.5",
            ProviderPolicy::CopilotOnly,
        )
        .unwrap();

        assert_eq!(route.qualified_model, "github-copilot:gpt-5.5");
        assert_eq!(route.producer.as_deref(), Some("openai"));
    }

    #[test]
    fn copilot_only_accepts_exact_copilot_route_pin() {
        let route = resolve_semantic_model_route(
            ModelRegistry::global(),
            "github-copilot:claude-sonnet-4.6",
            ProviderPolicy::CopilotOnly,
        )
        .unwrap();

        assert_eq!(route.qualified_model, "github-copilot:claude-sonnet-4.6");
        assert!(route.exact_route_pin);
    }

    #[test]
    fn copilot_only_rejects_exact_direct_route_pin() {
        let err = resolve_semantic_model_route(
            ModelRegistry::global(),
            "anthropic:claude-sonnet-4-6",
            ProviderPolicy::CopilotOnly,
        )
        .unwrap_err();

        assert_eq!(
            err,
            SemanticRouteResolutionError::ProviderNotAllowed {
                requested: "anthropic:claude-sonnet-4-6".into(),
                provider: "anthropic".into(),
                policy: ProviderPolicy::CopilotOnly,
            }
        );
    }

    #[test]
    fn direct_first_prefers_direct_route_for_conceptual_model() {
        let route = resolve_semantic_model_route(
            ModelRegistry::global(),
            "claude-sonnet-4.6",
            ProviderPolicy::DirectFirst,
        )
        .unwrap();

        assert_eq!(route.qualified_model, "anthropic:claude-sonnet-4-6");
    }
}
