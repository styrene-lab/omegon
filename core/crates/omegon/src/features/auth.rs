//! auth — Authentication status feature.
//!
//! Provides the `auth_status` tool for checking authentication status across
//! all backends (Anthropic OAuth, OpenAI OAuth, Vault, secrets store, MCP
//! remote, API keys). Injects authentication state as context and emits
//! notifications when credentials are about to expire.

use async_trait::async_trait;
use omegon_traits::{
    BusEvent, BusRequest, ContentBlock, ContextInjection, ContextSignals, Feature, NotifyLevel,
    ToolDefinition, ToolResult,
};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How often to check for credential expiry (in turns).
const EXPIRY_CHECK_INTERVAL: u32 = 5;

/// Authentication feature providing auth_status tool and context injection.
pub struct AuthFeature {
    /// Last turn when we checked for expiring credentials.
    last_expiry_check: u32,
    /// Cached provider status from last probe.
    cached_providers: Vec<crate::status::ProviderStatus>,
    /// Timestamp when providers were last probed.
    last_probe_time: Option<SystemTime>,
}

impl AuthFeature {
    pub fn new() -> Self {
        Self {
            last_expiry_check: 0,
            cached_providers: Vec::new(),
            last_probe_time: None,
        }
    }

    /// Probe all providers with caching (5 min TTL).
    async fn probe_providers_cached(&mut self) -> &[crate::status::ProviderStatus] {
        const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

        let now = SystemTime::now();
        let should_refresh = self
            .last_probe_time
            .is_none_or(|last| now.duration_since(last).unwrap_or(CACHE_TTL) >= CACHE_TTL);

        if should_refresh {
            let auth_status = crate::auth::probe_all_providers().await;
            self.cached_providers = crate::auth::auth_status_to_provider_statuses(&auth_status);
            self.last_probe_time = Some(now);
            tracing::debug!(
                providers = self.cached_providers.len(),
                "auth: refreshed provider cache"
            );
        }

        &self.cached_providers
    }
}

fn format_env_var_statuses(env_keys: &[&str]) -> String {
    let mut output = String::new();

    for key in env_keys {
        let status = if std::env::var(key).is_ok() {
            "Set"
        } else {
            "Not set"
        };
        output.push_str(&format!("- **{}:** {}\n", key, status));
    }

    output
}

#[async_trait]
impl Feature for AuthFeature {
    fn name(&self) -> &str {
        "auth"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: crate::tool_registry::auth::AUTH_STATUS.into(),
            label: "auth_status".into(),
            description: "Check authentication status across all backends (Anthropic OAuth, OpenAI OAuth, Vault, secrets store, MCP remote, API keys). Read-only status tool.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["status", "check"],
                        "default": "status",
                        "description": "Action to perform: 'status' for summary, 'check' for detailed probe"
                    }
                },
                "required": []
            }),
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        if tool_name != "auth_status" {
            anyhow::bail!("Unknown tool: {}", tool_name);
        }

        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("status");

        // For this tool we need to do a fresh probe since it's mutable self
        let auth_status = crate::auth::probe_all_providers().await;
        let providers = crate::auth::auth_status_to_provider_statuses(&auth_status);

        let mut output = String::new();

        match action {
            "status" => {
                output.push_str("# Authentication Status\n\n");

                if providers.is_empty() {
                    output.push_str("**No authentication providers configured.**\n");
                } else {
                    for provider in &providers {
                        let status_icon = if provider.authenticated { "✓" } else { "✗" };
                        let auth_method = provider
                            .auth_method
                            .as_ref()
                            .map(|s| format!(" ({})", s))
                            .unwrap_or_default();
                        let model_info = provider
                            .model
                            .as_ref()
                            .map(|m| format!(" → {}", m))
                            .unwrap_or_default();

                        output.push_str(&format!(
                            "{} **{}**{}{}\n",
                            status_icon, provider.name, auth_method, model_info
                        ));
                    }
                }

                // Check secrets store status
                if let Ok(secrets_path) = std::fs::canonicalize(
                    dirs::home_dir()
                        .unwrap_or_default()
                        .join(".omegon/secrets.db"),
                ) {
                    if secrets_path.exists() {
                        output.push_str("\n**Secrets Store:** Available (encrypted)\n");
                    }
                } else {
                    output.push_str("\n**Secrets Store:** Not initialized\n");
                }

                // Check Vault connectivity
                if let Ok(vault_output) = std::process::Command::new("vault")
                    .args(["status"])
                    .output()
                {
                    let vault_available = vault_output.status.success();
                    let vault_status = if vault_available {
                        "Connected"
                    } else {
                        "Disconnected"
                    };
                    output.push_str(&format!("\n**Vault:** {}\n", vault_status));
                }
            }
            "check" => {
                output.push_str("# Detailed Authentication Check\n\n");

                for provider in &providers {
                    output.push_str(&format!("## {}\n", provider.name));
                    output.push_str(&format!(
                        "- **Authenticated:** {}\n",
                        provider.authenticated
                    ));

                    if let Some(ref method) = provider.auth_method {
                        output.push_str(&format!("- **Method:** {}\n", method));
                    }

                    if let Some(ref model) = provider.model {
                        output.push_str(&format!("- **Active Model:** {}\n", model));
                    }

                    // Check token expiry for OAuth providers
                    if provider.auth_method.as_deref() == Some("oauth") {
                        let provider_lower = provider.name.to_lowercase();
                        let auth_key = crate::auth::auth_json_key(&provider_lower);

                        if let Some(creds) = crate::auth::read_credentials(auth_key) {
                            let expires_in = if creds.is_expired() {
                                "**Expired**".to_string()
                            } else {
                                let now_ms = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    as u64;
                                let remaining_ms = creds.expires.saturating_sub(now_ms);
                                let remaining_hours = remaining_ms / (1000 * 60 * 60);
                                format!("{}h", remaining_hours)
                            };
                            output.push_str(&format!("- **Token Expires:** {}\n", expires_in));
                        }
                    }

                    output.push('\n');
                }

                // Environment variables check
                output.push_str("## Environment Variables\n");
                let env_keys: Vec<&str> = crate::auth::PROVIDERS
                    .iter()
                    .flat_map(|provider| provider.env_vars.iter().copied())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();

                output.push_str(&format_env_var_statuses(&env_keys));
            }
            _ => {
                anyhow::bail!("Unknown action: {}", action);
            }
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text { text: output }],
            details: json!({
                "provider_count": providers.len(),
                "authenticated_count": providers.iter().filter(|p| p.authenticated).count()
            }),
        })
    }

    fn provide_context(&self, signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        // Only inject context if user prompt mentions auth-related terms
        let auth_keywords = [
            "auth",
            "login",
            "credential",
            "token",
            "oauth",
            "anthropic",
            "openai",
            "vault",
        ];

        let prompt_lower = signals.user_prompt.to_lowercase();
        if !auth_keywords.iter().any(|kw| prompt_lower.contains(kw)) {
            return None;
        }

        if self.cached_providers.is_empty() {
            return None;
        }

        let authenticated_providers: Vec<String> = self
            .cached_providers
            .iter()
            .filter(|p| p.authenticated)
            .map(|p| {
                let method = p
                    .auth_method
                    .as_ref()
                    .map(|s| format!(" ({})", s))
                    .unwrap_or_default();
                format!("{}{}", p.name, method)
            })
            .collect();

        if authenticated_providers.is_empty() {
            return Some(ContextInjection {
                source: "auth".into(),
                content: "[Auth] No authenticated providers available.".into(),
                priority: 5,
                ttl_turns: 1,
            });
        }

        let context = format!(
            "[Auth] Authenticated: {}",
            authenticated_providers.join(", ")
        );

        Some(ContextInjection {
            source: "auth".into(),
            content: context,
            priority: 5,
            ttl_turns: 1,
        })
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::TurnEnd { turn, .. } => {
                // Check for expiring credentials every N turns
                if turn.saturating_sub(self.last_expiry_check) >= EXPIRY_CHECK_INTERVAL {
                    self.last_expiry_check = *turn;
                    return self.check_expiring_credentials();
                }
            }
            BusEvent::ContextBuild { .. } => {
                // Refresh provider cache on context build
                crate::task_spawn::spawn_best_effort("auth-context-refresh", async {
                    let _providers = crate::auth::probe_all_providers().await;
                    // Cache would be updated here if we had a handle
                });
            }
            _ => {}
        }

        vec![]
    }
}

impl AuthFeature {
    /// Check for expiring OAuth credentials and emit warnings.
    fn check_expiring_credentials(&self) -> Vec<BusRequest> {
        let mut requests = Vec::new();

        for provider in &self.cached_providers {
            if provider.auth_method.as_deref() == Some("oauth") && provider.authenticated {
                let provider_lower = provider.name.to_lowercase();
                let auth_key = crate::auth::auth_json_key(&provider_lower);

                if let Some(creds) = crate::auth::read_credentials(auth_key) {
                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    let remaining_ms = creds.expires.saturating_sub(now_ms);
                    let remaining_hours = remaining_ms / (1000 * 60 * 60);

                    if creds.is_expired() {
                        requests.push(BusRequest::Notify {
                            message: format!("{} OAuth token has expired", provider.name),
                            level: NotifyLevel::Warning,
                        });
                    } else if remaining_hours < 24 {
                        requests.push(BusRequest::Notify {
                            message: format!(
                                "{} OAuth token expires in {}h",
                                provider.name, remaining_hours
                            ),
                            level: NotifyLevel::Info,
                        });
                    }
                }
            }
        }

        requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_name() {
        let feature = AuthFeature::new();
        assert_eq!(feature.name(), "auth");
    }

    #[test]
    fn provides_auth_status_tool() {
        let feature = AuthFeature::new();
        let tools = feature.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "auth_status");
    }

    #[test]
    fn format_env_var_statuses_reports_live_env_state() {
        let key = "OMEGON_TEST_AUTH_STATUS_ENV";
        // SAFETY: this test exclusively controls the variable and does not iterate env.
        unsafe { std::env::remove_var(key) };
        let unset = format_env_var_statuses(&[key]);
        assert!(unset.contains("Not set"));

        // SAFETY: this test exclusively controls the variable and does not iterate env.
        unsafe { std::env::set_var(key, "1") };
        let set = format_env_var_statuses(&[key]);
        assert!(set.contains("Set"));

        // SAFETY: cleanup for the same test-controlled variable.
        unsafe { std::env::remove_var(key) };
    }

    #[tokio::test]
    async fn auth_status_tool_execution() {
        let feature = AuthFeature::new();
        let result = feature
            .execute(
                "auth_status",
                "test-call",
                json!({"action": "status"}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(result.content.len(), 1);
        if let ContentBlock::Text { text } = &result.content[0] {
            assert!(text.contains("Authentication Status"));
        }
    }

    #[tokio::test]
    async fn auth_status_detailed_check() {
        let feature = AuthFeature::new();
        let result = feature
            .execute(
                "auth_status",
                "test-call",
                json!({"action": "check"}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(result.content.len(), 1);
        if let ContentBlock::Text { text } = &result.content[0] {
            assert!(text.contains("Detailed Authentication Check"));
            assert!(text.contains("Environment Variables"));
        }
    }

    #[test]
    fn context_injection_auth_keywords() {
        let mut feature = AuthFeature::new();
        feature.cached_providers = vec![crate::status::ProviderStatus {
            name: "Anthropic".into(),
            authenticated: true,
            auth_method: Some("oauth".into()),
            model: Some("claude-3-5-sonnet-20241022".into()),
            runtime_status: None,
            recent_failure_count: None,
            last_failure_kind: None,
            last_failure_at: None,
        }];

        let signals = ContextSignals {
            user_prompt: "How do I login to anthropic?",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 1000,
        };

        let injection = feature.provide_context(&signals);
        assert!(injection.is_some());

        let injection = injection.unwrap();
        assert_eq!(injection.source, "auth");
        assert!(injection.content.contains("Authenticated"));
    }

    #[test]
    fn no_context_injection_without_auth_keywords() {
        let feature = AuthFeature::new();
        let signals = ContextSignals {
            user_prompt: "Write a hello world program",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 1000,
        };

        let injection = feature.provide_context(&signals);
        assert!(injection.is_none());
    }

    #[test]
    fn expiry_check_interval() {
        let mut feature = AuthFeature::new();
        feature.last_expiry_check = 0;

        // First check after interval should trigger
        let _requests = feature.on_event(&BusEvent::TurnEnd {
            turn: EXPIRY_CHECK_INTERVAL,
            model: None,
            provider: None,
            estimated_tokens: 0,
            context_window: 200_000,
            context_composition: omegon_traits::ContextComposition::default(),
            actual_input_tokens: 0,
            actual_output_tokens: 0,
            cache_read_tokens: 0,
            provider_telemetry: None,
        });
        // Will be empty since no cached providers, but interval logic should work
        assert_eq!(feature.last_expiry_check, EXPIRY_CHECK_INTERVAL);

        // Immediate subsequent check should not trigger
        let _requests2 = feature.on_event(&BusEvent::TurnEnd {
            turn: EXPIRY_CHECK_INTERVAL + 1,
            model: None,
            provider: None,
            estimated_tokens: 0,
            context_window: 200_000,
            context_composition: omegon_traits::ContextComposition::default(),
            actual_input_tokens: 0,
            actual_output_tokens: 0,
            cache_read_tokens: 0,
            provider_telemetry: None,
        });
        assert_eq!(feature.last_expiry_check, EXPIRY_CHECK_INTERVAL); // unchanged
    }

    #[test]
    fn auth_probe_to_harness_status_pipeline() {
        // C2: end-to-end test for auth → convert → HarnessStatus.providers
        use crate::auth::{
            AuthStatus, ProviderAuthStatus, ProviderInfo, auth_status_to_provider_statuses,
        };

        let status = AuthStatus {
            providers: vec![
                ProviderInfo {
                    name: "anthropic".into(),
                    status: ProviderAuthStatus::Authenticated,
                    is_oauth: true,
                    details: Some("oauth".into()),
                },
                ProviderInfo {
                    name: "openai".into(),
                    status: ProviderAuthStatus::Missing,
                    is_oauth: false,
                    details: None,
                },
            ],
            vault: vec![],
            secrets: vec![],
            mcp: vec![],
        };

        // Convert to ProviderStatus (what HarnessStatus uses)
        let providers = auth_status_to_provider_statuses(&status);
        assert_eq!(providers.len(), 2);

        // Verify first provider
        assert_eq!(providers[0].name, "anthropic");
        assert!(providers[0].authenticated);
        assert_eq!(providers[0].auth_method, Some("oauth".into()));

        // Verify second provider
        assert_eq!(providers[1].name, "openai");
        assert!(!providers[1].authenticated);

        // Simulate wiring into HarnessStatus (as setup.rs does)
        let mut harness = crate::status::HarnessStatus::default();
        harness.providers = providers;
        assert_eq!(harness.providers.len(), 2);
        assert!(harness.providers[0].authenticated);
        assert!(!harness.providers[1].authenticated);

        // Verify it serializes for WebSocket broadcast
        let json = serde_json::to_value(&harness).unwrap();
        let ws_providers = json["providers"].as_array().unwrap();
        assert_eq!(ws_providers.len(), 2);
        assert_eq!(ws_providers[0]["name"].as_str().unwrap(), "anthropic");
    }
}
