//! Validated provider contributions for inference routing.
//!
//! Declarations bind provider semantics without owning request routing. Slice
//! 4.2 consumes this registry through one route service and durable route lease.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use omegon_traits::{RuntimeContributionGenerationId, RuntimeContributionId};
use serde::{Deserialize, Serialize};

use crate::tool_schema::SchemaDialect;

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderAuthenticationClass {
    CredentiallessLocal,
    AnonymousHosted,
    ApiKey,
    OAuth,
    ApiKeyOrOAuth,
    OAuthTokenExchange,
    OptionalApiKeyLocal,
}

impl ProviderAuthenticationClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::CredentiallessLocal => "credentialless_local",
            Self::AnonymousHosted => "anonymous_hosted",
            Self::ApiKey => "api_key",
            Self::OAuth => "oauth",
            Self::ApiKeyOrOAuth => "api_key_or_oauth",
            Self::OAuthTokenExchange => "oauth_token_exchange",
            Self::OptionalApiKeyLocal => "optional_api_key_local",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "support", content = "dialect", rename_all = "snake_case")]
pub(crate) enum ProviderToolContract {
    Supported(SchemaDialect),
    Unsupported,
}

impl ProviderToolContract {
    pub(crate) fn dialect_name(self) -> &'static str {
        match self {
            Self::Supported(SchemaDialect::Anthropic) => "anthropic",
            Self::Supported(SchemaDialect::Full) => "full",
            Self::Supported(SchemaDialect::OpenAI) => "open_ai",
            Self::Supported(SchemaDialect::Gemini) => "gemini",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderBridgeFactoryBinding {
    AnthropicMessages,
    OpenAiApi,
    OpenAiChatCompletions,
    OpenAiResponses,
    OpenRouter,
    GithubCopilot,
    GoogleAntigravity,
    OllamaCloud,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "authority", rename_all = "snake_case")]
pub(crate) enum ProviderInventoryAuthority {
    RuntimeInferenceInventory { provider_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderEvidenceBinding {
    OfferingModalitiesAndCapabilities,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub(crate) enum ProviderContinuityPolicy {
    #[default]
    None,
    RestrictedRequired {
        allowed_kinds: Vec<crate::bridge::ProviderContinuityKind>,
        max_blob_bytes: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderModelFamily {
    OpenAiNative,
    GoogleGemini,
}

impl ProviderModelFamily {
    fn contains(self, model_id: &str) -> bool {
        let model_id = model_id.to_ascii_lowercase();
        match self {
            Self::OpenAiNative => {
                model_id.starts_with("gpt-")
                    || model_id == "o1"
                    || model_id == "o3"
                    || model_id == "o4"
                    || model_id.starts_with("o1-")
                    || model_id.starts_with("o3-")
                    || model_id.starts_with("o4-")
            }
            Self::GoogleGemini => model_id.starts_with("gemini"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderFallbackCompatibility {
    pub target_provider_id: String,
    pub model_family: ProviderModelFamily,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderContribution {
    pub schema_version: u16,
    pub provider_id: String,
    pub aliases: Vec<String>,
    pub owner_id: RuntimeContributionId,
    pub owner_generation_id: RuntimeContributionGenerationId,
    pub inventory: ProviderInventoryAuthority,
    pub authentication: ProviderAuthenticationClass,
    pub tools: ProviderToolContract,
    pub bridge_factory: ProviderBridgeFactoryBinding,
    pub evidence: ProviderEvidenceBinding,
    pub continuity: ProviderContinuityPolicy,
    pub fallback_compatibility: Vec<ProviderFallbackCompatibility>,
    pub executable: bool,
}

#[derive(Debug, Clone, Default)]
struct ProviderContributionCandidate {
    provider_id: String,
    aliases: Vec<String>,
    owner_id: Option<RuntimeContributionId>,
    owner_generation_id: Option<RuntimeContributionGenerationId>,
    inventory: Option<ProviderInventoryAuthority>,
    authentication: Option<ProviderAuthenticationClass>,
    tools: Option<ProviderToolContract>,
    bridge_factory: Option<ProviderBridgeFactoryBinding>,
    evidence: Option<ProviderEvidenceBinding>,
    continuity: ProviderContinuityPolicy,
    fallback_compatibility: Vec<ProviderFallbackCompatibility>,
    executable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderContributionDiagnostic {
    pub provider_id: String,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub(crate) struct ProviderContributionRegistry {
    providers: BTreeMap<String, ProviderContribution>,
    aliases: BTreeMap<String, String>,
}

impl ProviderContributionRegistry {
    fn build(
        mut candidates: Vec<ProviderContributionCandidate>,
    ) -> Result<Self, Vec<ProviderContributionDiagnostic>> {
        candidates.sort_by(|left, right| left.provider_id.cmp(&right.provider_id));
        let duplicate_ids = candidates
            .iter()
            .map(|candidate| candidate.provider_id.trim().to_ascii_lowercase())
            .fold(
                BTreeMap::<String, usize>::new(),
                |mut counts, provider_id| {
                    *counts.entry(provider_id).or_default() += 1;
                    counts
                },
            )
            .into_iter()
            .filter_map(|(provider_id, count)| (count > 1).then_some(provider_id))
            .collect::<BTreeSet<_>>();
        let mut reported_duplicate_ids = BTreeSet::new();
        let mut diagnostics = Vec::new();
        let mut providers = BTreeMap::new();
        let mut aliases = BTreeMap::new();

        for candidate in candidates {
            let provider_id = candidate.provider_id.trim().to_ascii_lowercase();
            if provider_id.is_empty()
                || !provider_id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            {
                diagnostics.push(diagnostic(
                    &provider_id,
                    "provider:invalid_id",
                    "provider identity must be a non-empty lowercase scoped token",
                ));
                continue;
            }
            if duplicate_ids.contains(&provider_id) {
                if reported_duplicate_ids.insert(provider_id.clone()) {
                    diagnostics.push(diagnostic(
                        &provider_id,
                        "provider:duplicate_id",
                        "provider identity is declared more than once",
                    ));
                }
                continue;
            }
            if aliases.contains_key(&provider_id) {
                diagnostics.push(diagnostic(
                    &provider_id,
                    "provider:duplicate_id",
                    "provider identity is declared more than once",
                ));
                continue;
            }

            let owner_id = required(
                &provider_id,
                "owner_id",
                candidate.owner_id,
                &mut diagnostics,
            );
            let owner_generation_id = required(
                &provider_id,
                "owner_generation_id",
                candidate.owner_generation_id,
                &mut diagnostics,
            );
            let inventory = required(
                &provider_id,
                "inventory",
                candidate.inventory,
                &mut diagnostics,
            );
            let authentication = required(
                &provider_id,
                "authentication",
                candidate.authentication,
                &mut diagnostics,
            );
            let tools = required(&provider_id, "tools", candidate.tools, &mut diagnostics);
            let bridge_factory = required(
                &provider_id,
                "bridge_factory",
                candidate.bridge_factory,
                &mut diagnostics,
            );
            let evidence = required(
                &provider_id,
                "evidence",
                candidate.evidence,
                &mut diagnostics,
            );
            let (
                Some(owner_id),
                Some(owner_generation_id),
                Some(inventory),
                Some(authentication),
                Some(tools),
                Some(bridge_factory),
                Some(evidence),
            ) = (
                owner_id,
                owner_generation_id,
                inventory,
                authentication,
                tools,
                bridge_factory,
                evidence,
            )
            else {
                continue;
            };
            if !factory_accepts_authentication(bridge_factory, authentication) {
                diagnostics.push(diagnostic(
                    &provider_id,
                    "provider:factory_auth_mismatch",
                    "bridge factory does not accept the declared authentication class",
                ));
                continue;
            }
            if let ProviderContinuityPolicy::RestrictedRequired {
                allowed_kinds,
                max_blob_bytes,
            } = &candidate.continuity
                && (allowed_kinds.is_empty()
                    || allowed_kinds.windows(2).any(|pair| pair[0] >= pair[1])
                    || *max_blob_bytes == 0
                    || *max_blob_bytes > crate::session_blob_store::MAX_SESSION_BLOB_BYTES)
            {
                diagnostics.push(diagnostic(
                    &provider_id,
                    "provider:invalid_continuity_policy",
                    "restricted continuity kinds must be sorted and unique with a bounded non-zero size ceiling",
                ));
                continue;
            }

            let ProviderInventoryAuthority::RuntimeInferenceInventory {
                provider_id: inventory_provider_id,
            } = &inventory;
            if inventory_provider_id != &provider_id {
                diagnostics.push(diagnostic(
                    &provider_id,
                    "provider:inventory_owner_mismatch",
                    "runtime inventory authority must retain provider identity",
                ));
                continue;
            }

            let mut normalized_aliases = Vec::new();
            for alias in candidate.aliases {
                let alias = alias.trim().to_ascii_lowercase();
                if alias.is_empty() || alias == provider_id {
                    diagnostics.push(diagnostic(
                        &provider_id,
                        "provider:invalid_alias",
                        "provider aliases must be non-empty and distinct from the canonical id",
                    ));
                    continue;
                }
                if providers.contains_key(&alias) || aliases.contains_key(&alias) {
                    diagnostics.push(diagnostic(
                        &provider_id,
                        "provider:duplicate_alias",
                        format!("provider alias {alias} is already owned"),
                    ));
                    continue;
                }
                aliases.insert(alias.clone(), provider_id.clone());
                normalized_aliases.push(alias);
            }

            providers.insert(
                provider_id.clone(),
                ProviderContribution {
                    schema_version: SCHEMA_VERSION,
                    provider_id,
                    aliases: normalized_aliases,
                    owner_id,
                    owner_generation_id,
                    inventory,
                    authentication,
                    tools,
                    bridge_factory,
                    evidence,
                    continuity: candidate.continuity,
                    fallback_compatibility: candidate.fallback_compatibility,
                    executable: candidate.executable,
                },
            );
        }

        for contribution in providers.values() {
            for fallback in &contribution.fallback_compatibility {
                if fallback.target_provider_id == contribution.provider_id
                    || !providers.contains_key(&fallback.target_provider_id)
                {
                    diagnostics.push(diagnostic(
                        &contribution.provider_id,
                        "provider:dangling_fallback",
                        format!(
                            "fallback target {} is missing or self-referential",
                            fallback.target_provider_id
                        ),
                    ));
                }
            }
        }

        if diagnostics.is_empty() {
            Ok(Self { providers, aliases })
        } else {
            diagnostics.sort_by(|left, right| {
                (&left.provider_id, left.code, &left.message).cmp(&(
                    &right.provider_id,
                    right.code,
                    &right.message,
                ))
            });
            Err(diagnostics)
        }
    }

    pub(crate) fn get(&self, provider_id: &str) -> Option<&ProviderContribution> {
        let provider_id = provider_id.trim().to_ascii_lowercase();
        let canonical = self.aliases.get(&provider_id).unwrap_or(&provider_id);
        self.providers.get(canonical)
    }

    pub(crate) fn fallback_targets<'a>(
        &'a self,
        provider_id: &str,
        model_id: &str,
    ) -> impl Iterator<Item = &'a str> {
        self.get(provider_id)
            .into_iter()
            .flat_map(|provider| provider.fallback_compatibility.iter())
            .filter(move |fallback| fallback.model_family.contains(model_id))
            .map(|fallback| fallback.target_provider_id.as_str())
    }

    pub(crate) fn inventory_diagnostics(
        &self,
        snapshot: &crate::inference_inventory::InventorySnapshot,
    ) -> Vec<ProviderContributionDiagnostic> {
        let mut diagnostics = Vec::new();
        for provider in self.providers.values() {
            let offerings = snapshot.offerings.values().filter(|offering| {
                snapshot
                    .endpoints
                    .get(&offering.endpoint.value)
                    .map(|endpoint| {
                        endpoint
                            .group
                            .as_ref()
                            .map_or(endpoint.id.0.as_str(), |group| group.value.0.as_str())
                    })
                    .and_then(|provider_id| self.get(provider_id))
                    .is_some_and(|candidate| candidate.provider_id == provider.provider_id)
            });
            let mut found = false;
            for offering in offerings {
                found = true;
                if offering.input_modalities.value.is_empty()
                    || offering.output_modalities.value.is_empty()
                {
                    diagnostics.push(diagnostic(
                        &provider.provider_id,
                        "provider:missing_modality_evidence",
                        format!(
                            "offering {} lacks input or output modality evidence",
                            offering.id.0
                        ),
                    ));
                }
                if offering.capabilities.is_empty() {
                    diagnostics.push(diagnostic(
                        &provider.provider_id,
                        "provider:missing_capability_evidence",
                        format!("offering {} lacks capability evidence", offering.id.0),
                    ));
                }
            }
            if !found {
                diagnostics.push(diagnostic(
                    &provider.provider_id,
                    "provider:missing_inventory",
                    "runtime inference inventory has no offering for this provider",
                ));
            }
        }
        diagnostics.sort_by(|left, right| {
            (&left.provider_id, left.code, &left.message).cmp(&(
                &right.provider_id,
                right.code,
                &right.message,
            ))
        });
        diagnostics
    }
}

fn factory_accepts_authentication(
    factory: ProviderBridgeFactoryBinding,
    authentication: ProviderAuthenticationClass,
) -> bool {
    use ProviderAuthenticationClass as Auth;
    use ProviderBridgeFactoryBinding as Factory;

    match factory {
        Factory::AnthropicMessages => authentication == Auth::ApiKeyOrOAuth,
        Factory::OpenAiApi | Factory::OpenRouter | Factory::OllamaCloud => {
            authentication == Auth::ApiKey
        }
        Factory::OpenAiResponses | Factory::GoogleAntigravity => authentication == Auth::OAuth,
        Factory::GithubCopilot => authentication == Auth::OAuthTokenExchange,
        Factory::OpenAiChatCompletions => matches!(
            authentication,
            Auth::ApiKey
                | Auth::CredentiallessLocal
                | Auth::OptionalApiKeyLocal
                | Auth::AnonymousHosted
        ),
    }
}

fn required<T>(
    provider_id: &str,
    field: &'static str,
    value: Option<T>,
    diagnostics: &mut Vec<ProviderContributionDiagnostic>,
) -> Option<T> {
    if value.is_none() {
        diagnostics.push(diagnostic(
            provider_id,
            "provider:missing_semantics",
            format!("provider contribution is missing {field}"),
        ));
    }
    value
}

fn diagnostic(
    provider_id: &str,
    code: &'static str,
    message: impl Into<String>,
) -> ProviderContributionDiagnostic {
    ProviderContributionDiagnostic {
        provider_id: provider_id.to_string(),
        code,
        message: message.into(),
    }
}

fn candidate(
    provider_id: &str,
    aliases: &[&str],
    authentication: ProviderAuthenticationClass,
    tools: ProviderToolContract,
    bridge_factory: ProviderBridgeFactoryBinding,
) -> ProviderContributionCandidate {
    ProviderContributionCandidate {
        provider_id: provider_id.into(),
        aliases: aliases.iter().map(|alias| (*alias).into()).collect(),
        owner_id: Some(
            RuntimeContributionId::new(format!("provider:{provider_id}"))
                .expect("built-in provider owner id must be valid"),
        ),
        owner_generation_id: Some(
            RuntimeContributionGenerationId::new(format!("provider:{provider_id}/builtin-v1"))
                .expect("built-in provider generation id must be valid"),
        ),
        inventory: Some(ProviderInventoryAuthority::RuntimeInferenceInventory {
            provider_id: provider_id.into(),
        }),
        authentication: Some(authentication),
        tools: Some(tools),
        bridge_factory: Some(bridge_factory),
        evidence: Some(ProviderEvidenceBinding::OfferingModalitiesAndCapabilities),
        continuity: ProviderContinuityPolicy::None,
        fallback_compatibility: Vec::new(),
        executable: true,
    }
}

fn fallback(
    target_provider_id: &str,
    model_family: ProviderModelFamily,
) -> ProviderFallbackCompatibility {
    ProviderFallbackCompatibility {
        target_provider_id: target_provider_id.into(),
        model_family,
    }
}

fn built_in_candidates() -> Vec<ProviderContributionCandidate> {
    use ProviderAuthenticationClass as Auth;
    use ProviderBridgeFactoryBinding as Factory;
    use ProviderToolContract as Tools;

    let openai_tools = Tools::Supported(SchemaDialect::OpenAI);
    vec![
        ProviderContributionCandidate {
            continuity: ProviderContinuityPolicy::RestrictedRequired {
                allowed_kinds: vec![crate::bridge::ProviderContinuityKind::HiddenReasoning],
                max_blob_bytes: 64 * 1024,
            },
            ..candidate(
                "anthropic",
                &["claude"],
                Auth::ApiKeyOrOAuth,
                Tools::Supported(SchemaDialect::Anthropic),
                Factory::AnthropicMessages,
            )
        },
        ProviderContributionCandidate {
            fallback_compatibility: vec![fallback(
                "openai-codex",
                ProviderModelFamily::OpenAiNative,
            )],
            ..candidate(
                "openai",
                &[],
                Auth::ApiKey,
                openai_tools,
                Factory::OpenAiApi,
            )
        },
        ProviderContributionCandidate {
            fallback_compatibility: vec![fallback("openai", ProviderModelFamily::OpenAiNative)],
            ..candidate(
                "openai-codex",
                &["chatgpt", "codex"],
                Auth::OAuth,
                openai_tools,
                Factory::OpenAiResponses,
            )
        },
        candidate(
            "github-copilot",
            &["copilot"],
            Auth::OAuthTokenExchange,
            openai_tools,
            Factory::GithubCopilot,
        ),
        candidate(
            "openrouter",
            &[],
            Auth::ApiKey,
            openai_tools,
            Factory::OpenRouter,
        ),
        candidate(
            "groq",
            &[],
            Auth::ApiKey,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "xai",
            &[],
            Auth::ApiKey,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "mistral",
            &[],
            Auth::ApiKey,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "cerebras",
            &[],
            Auth::ApiKey,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "moonshot",
            &["kimi"],
            Auth::ApiKey,
            Tools::Supported(SchemaDialect::Full),
            Factory::OpenAiChatCompletions,
        ),
        ProviderContributionCandidate {
            fallback_compatibility: vec![fallback(
                "google-antigravity",
                ProviderModelFamily::GoogleGemini,
            )],
            ..candidate(
                "google",
                &["gemini"],
                Auth::ApiKey,
                openai_tools,
                Factory::OpenAiChatCompletions,
            )
        },
        ProviderContributionCandidate {
            fallback_compatibility: vec![fallback("google", ProviderModelFamily::GoogleGemini)],
            executable: false,
            ..candidate(
                "google-antigravity",
                &["antigravity"],
                Auth::OAuth,
                Tools::Supported(SchemaDialect::Gemini),
                Factory::GoogleAntigravity,
            )
        },
        candidate(
            "opencode-zen",
            &["zen"],
            Auth::AnonymousHosted,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "opencode-go",
            &[],
            Auth::ApiKey,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "perplexity",
            &[],
            Auth::ApiKey,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "huggingface",
            &[],
            Auth::ApiKey,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "ollama",
            &["local"],
            Auth::CredentiallessLocal,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
        candidate(
            "ollama-cloud",
            &[],
            Auth::ApiKey,
            Tools::Unsupported,
            Factory::OllamaCloud,
        ),
        candidate(
            "dwarfstar",
            &[],
            Auth::OptionalApiKeyLocal,
            openai_tools,
            Factory::OpenAiChatCompletions,
        ),
    ]
}

static PROVIDERS: LazyLock<ProviderContributionRegistry> = LazyLock::new(|| {
    ProviderContributionRegistry::build(built_in_candidates()).unwrap_or_else(|diagnostics| {
        panic!("built-in provider contributions are invalid: {diagnostics:#?}")
    })
});

pub(crate) fn registry() -> &'static ProviderContributionRegistry {
    &PROVIDERS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn valid_candidate(provider_id: &str) -> ProviderContributionCandidate {
        candidate(
            provider_id,
            &[],
            ProviderAuthenticationClass::ApiKey,
            ProviderToolContract::Supported(SchemaDialect::OpenAI),
            ProviderBridgeFactoryBinding::OpenAiChatCompletions,
        )
    }

    #[test]
    fn incomplete_provider_reports_each_missing_semantic() {
        let incomplete = ProviderContributionCandidate {
            provider_id: "incomplete".into(),
            bridge_factory: Some(ProviderBridgeFactoryBinding::OpenAiChatCompletions),
            ..ProviderContributionCandidate::default()
        };
        let diagnostics = ProviderContributionRegistry::build(vec![incomplete]).unwrap_err();
        let messages = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<BTreeSet<_>>();
        for field in [
            "owner_id",
            "owner_generation_id",
            "inventory",
            "authentication",
            "tools",
            "evidence",
        ] {
            assert!(
                messages.iter().any(|message| message.contains(field)),
                "missing diagnostic for {field}: {diagnostics:#?}"
            );
        }
    }

    #[test]
    fn duplicate_alias_fails_closed() {
        let first = ProviderContributionCandidate {
            aliases: vec!["shared".into()],
            ..valid_candidate("first")
        };
        let duplicate_alias = ProviderContributionCandidate {
            aliases: vec!["shared".into()],
            ..valid_candidate("second")
        };
        let diagnostics =
            ProviderContributionRegistry::build(vec![first, duplicate_alias]).unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "provider:duplicate_alias")
        );
    }

    #[test]
    fn canonical_identity_cannot_claim_an_earlier_alias() {
        let alias_owner = ProviderContributionCandidate {
            aliases: vec!["claimed".into()],
            ..valid_candidate("owner")
        };
        let diagnostics =
            ProviderContributionRegistry::build(vec![alias_owner, valid_candidate("claimed")])
                .unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "provider:duplicate_alias")
        );
    }

    #[test]
    fn collision_diagnostics_are_registration_order_independent() {
        let candidates = || {
            vec![
                ProviderContributionCandidate {
                    aliases: vec!["claimed".into()],
                    ..valid_candidate("owner")
                },
                valid_candidate("claimed"),
            ]
        };
        let forward = ProviderContributionRegistry::build(candidates()).unwrap_err();
        let mut reversed = candidates();
        reversed.reverse();
        assert_eq!(
            ProviderContributionRegistry::build(reversed).unwrap_err(),
            forward
        );
    }

    #[test]
    fn duplicate_id_diagnostics_ignore_candidate_order_and_completeness() {
        let valid = valid_candidate("duplicate");
        let incomplete = ProviderContributionCandidate {
            provider_id: "duplicate".into(),
            ..ProviderContributionCandidate::default()
        };
        let forward = ProviderContributionRegistry::build(vec![valid.clone(), incomplete.clone()])
            .unwrap_err();
        let reversed = ProviderContributionRegistry::build(vec![incomplete, valid]).unwrap_err();
        assert_eq!(forward, reversed);
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].code, "provider:duplicate_id");
    }

    #[test]
    fn factory_and_authentication_must_agree() {
        let mismatched = ProviderContributionCandidate {
            authentication: Some(ProviderAuthenticationClass::OAuth),
            ..valid_candidate("mismatched")
        };
        let diagnostics = ProviderContributionRegistry::build(vec![mismatched]).unwrap_err();
        assert_eq!(diagnostics[0].code, "provider:factory_auth_mismatch");
    }

    #[test]
    fn dangling_fallback_fails_closed() {
        let candidate = ProviderContributionCandidate {
            fallback_compatibility: vec![fallback("missing", ProviderModelFamily::OpenAiNative)],
            ..valid_candidate("source")
        };
        let diagnostics = ProviderContributionRegistry::build(vec![candidate]).unwrap_err();
        assert_eq!(diagnostics[0].code, "provider:dangling_fallback");
    }

    #[test]
    fn built_in_contributions_cover_constructible_provider_ids() {
        let expected = BTreeSet::from([
            "anthropic",
            "cerebras",
            "dwarfstar",
            "github-copilot",
            "google",
            "google-antigravity",
            "groq",
            "huggingface",
            "mistral",
            "moonshot",
            "ollama",
            "ollama-cloud",
            "openai",
            "openai-codex",
            "opencode-go",
            "opencode-zen",
            "openrouter",
            "perplexity",
            "xai",
        ]);
        assert_eq!(
            registry()
                .providers
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            expected
        );
    }

    #[test]
    fn fallback_is_declared_and_model_family_bounded() {
        assert_eq!(
            registry()
                .fallback_targets("openai", "gpt-5.6")
                .collect::<Vec<_>>(),
            vec!["openai-codex"]
        );
        assert_eq!(
            registry()
                .fallback_targets("openai", "claude-sonnet-4-6")
                .collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
        assert_eq!(
            registry()
                .fallback_targets("google", "gemini-3-pro")
                .collect::<Vec<_>>(),
            vec!["google-antigravity"]
        );
        assert_eq!(
            registry()
                .fallback_targets("openrouter", "openai/gpt-5.6")
                .collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn executable_tool_contract_matches_current_adapters() {
        assert_eq!(
            registry().get("google").unwrap().tools,
            ProviderToolContract::Supported(SchemaDialect::OpenAI)
        );
        assert_eq!(
            registry().get("moonshot").unwrap().tools,
            ProviderToolContract::Supported(SchemaDialect::Full)
        );
        assert_eq!(
            registry().get("ollama-cloud").unwrap().tools,
            ProviderToolContract::Unsupported
        );
        assert!(!registry().get("google-antigravity").unwrap().executable);
    }

    #[test]
    fn inventory_binding_requires_real_offering_evidence() {
        let diagnostics = registry()
            .inventory_diagnostics(&crate::inference_inventory::InventorySnapshot::empty());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.provider_id == "openai"
                    && diagnostic.code == "provider:missing_inventory")
        );
    }

    #[test]
    fn inventory_binding_uses_endpoint_owner_not_offering_name() {
        use crate::inference_inventory::{
            AdapterId, EndpointGroupId, EndpointGroupPatch, EndpointId, EndpointPatch,
            EvidenceKind, InventoryLayer, InventorySnapshot, InventorySource, Modality, OfferingId,
            OfferingPatch, TransportSpec,
        };

        let mut layer = InventoryLayer::new(InventorySource::Project, EvidenceKind::Declared);
        layer.providers.insert(
            EndpointGroupId("openrouter".into()),
            EndpointGroupPatch {
                display_name: Some("OpenRouter".into()),
            },
        );
        layer.endpoints.insert(
            EndpointId("broker".into()),
            EndpointPatch {
                group: Some(Some(EndpointGroupId("openrouter".into()))),
                adapter: Some(AdapterId(AdapterId::CHAT_COMPLETIONS.into())),
                transport: Some(TransportSpec::Http {
                    base_url: "https://example.invalid".into(),
                }),
                enabled: Some(true),
                ..Default::default()
            },
        );
        layer.offerings.insert(
            OfferingId("openai:misleading-name".into()),
            OfferingPatch {
                endpoint: Some(EndpointId("broker".into())),
                native_model_id: Some("misleading-name".into()),
                input_modalities: Some([Modality(Modality::TEXT.into())].into()),
                output_modalities: Some([Modality(Modality::TEXT.into())].into()),
                capabilities: BTreeMap::from([("tools".into(), true)]),
                ..Default::default()
            },
        );
        let snapshot = InventorySnapshot::build(1, vec![layer]).unwrap();
        let diagnostics = registry().inventory_diagnostics(&snapshot);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.provider_id == "openai"
                    && diagnostic.code == "provider:missing_inventory")
        );
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic.provider_id == "openrouter"
                && diagnostic.code == "provider:missing_inventory"
        }));
    }

    #[test]
    fn built_in_inventory_reports_only_providers_without_offerings() {
        let snapshot = crate::inference_inventory::InventorySnapshot::build(
            1,
            vec![
                crate::inference_inventory::InventoryLayer::embedded_registry(
                    crate::model_registry::ModelRegistry::global(),
                ),
            ],
        )
        .unwrap();
        let ox_alpha = snapshot
            .offerings
            .get(&crate::inference_inventory::OfferingId(
                "openrouter:stealth/ox-alpha".into(),
            ))
            .expect("Ox Alpha should project into runtime inventory");
        for modality in ["text", "image", "video"] {
            assert!(
                ox_alpha
                    .input_modalities
                    .value
                    .contains(&crate::inference_inventory::Modality(modality.into()))
            );
        }
        assert_eq!(
            ox_alpha.output_modalities.value,
            [crate::inference_inventory::Modality("text".into())].into()
        );
        let diagnostics = registry().inventory_diagnostics(&snapshot);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "provider:missing_inventory"),
            "embedded offerings should carry complete evidence: {diagnostics:#?}"
        );
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.provider_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "cerebras",
                "dwarfstar",
                "google-antigravity",
                "huggingface",
                "ollama",
            ])
        );
    }

    #[test]
    fn credential_catalog_covers_provider_authentication_bindings() {
        for provider in registry().providers.values() {
            let credential =
                crate::auth::provider_by_id(&provider.provider_id).unwrap_or_else(|| {
                    panic!("missing credential descriptor for {}", provider.provider_id)
                });
            let compatible = match provider.authentication {
                ProviderAuthenticationClass::AnonymousHosted => {
                    credential.auth_method == crate::auth::AuthMethod::Anonymous
                        && credential.env_vars.is_empty()
                }
                ProviderAuthenticationClass::ApiKey => {
                    credential.auth_method == crate::auth::AuthMethod::ApiKey
                        && credential.oauth_env_vars.is_empty()
                }
                ProviderAuthenticationClass::OAuth
                | ProviderAuthenticationClass::OAuthTokenExchange => {
                    credential.auth_method == crate::auth::AuthMethod::OAuth
                        && !credential.oauth_env_vars.is_empty()
                }
                ProviderAuthenticationClass::ApiKeyOrOAuth => {
                    credential.auth_method == crate::auth::AuthMethod::OAuth
                        && !credential.oauth_env_vars.is_empty()
                        && credential
                            .env_vars
                            .iter()
                            .any(|name| !credential.oauth_env_vars.contains(name))
                }
                ProviderAuthenticationClass::CredentiallessLocal
                | ProviderAuthenticationClass::OptionalApiKeyLocal => {
                    credential.auth_method == crate::auth::AuthMethod::ApiKey
                }
            };
            assert!(
                compatible,
                "credential UX class disagrees with execution auth for {}",
                provider.provider_id
            );
        }
    }

    #[test]
    fn serialized_contribution_is_stable_and_complete() {
        let encoded = serde_json::to_value(registry().get("openai").unwrap()).unwrap();
        assert_eq!(encoded["schema_version"], 1);
        assert_eq!(encoded["provider_id"], "openai");
        assert_eq!(
            encoded["inventory"]["authority"],
            "runtime_inference_inventory"
        );
        assert_eq!(encoded["authentication"], "api_key");
        assert_eq!(encoded["tools"]["support"], "supported");
        assert_eq!(encoded["bridge_factory"], "open_ai_api");
        assert_eq!(encoded["evidence"], "offering_modalities_and_capabilities");
    }
}
