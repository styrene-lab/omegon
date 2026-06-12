//! Provider route state machine.
//!
//! This module is the authoritative model for "what provider/model is serving
//! this interactive session?". It intentionally contains no TUI types; UI,
//! loop, web, and daemon-facing surfaces consume [`RouteSnapshot`] instead.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::{broadcast, RwLock};

use crate::bridge::{LlmBridge, NullBridge};

/// Why the selected model is not the one serving the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    MissingCredentials { provider: String },
    ExpiredCredentials { provider: String },
    ProviderUnavailable { provider: String, detail: String },
}

/// Why no route can currently serve requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisconnectedReason {
    MissingCredentials {
        provider: String,
        probed_sources: Vec<String>,
    },
    ExpiredCredentials {
        provider: String,
        refreshable: bool,
    },
    FallbackExhausted {
        selected: String,
        attempts: Vec<ProviderAttempt>,
    },
    ProviderUnavailable {
        provider: String,
        detail: String,
    },
}

/// One configured fallback provider attempt and why it did or did not work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderAttempt {
    pub provider: String,
    pub state: CredentialState,
}

/// Terminal outcome for an interactive login attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginFailureReason {
    Timeout,
    StaleStateOnly,
    Refused(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginOutcome {
    Succeeded { model: String },
    Failed { reason: LoginFailureReason },
}

/// Provider credential state as seen by the route resolver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialState {
    Valid { source: CredentialSource, oauth: bool },
    Expired { source: CredentialSource, refreshable: bool },
    Missing { probed_sources: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialSource {
    Environment,
    AuthJson,
    External,
}

impl CredentialState {
    pub fn is_valid(&self) -> bool {
        matches!(self, CredentialState::Valid { .. })
    }
}

/// Re-probing credential ledger. It does not cache: external tools mutate their
/// credential files outside our process.
#[derive(Clone, Debug, Default)]
pub struct CredentialLedger;

impl CredentialLedger {
    pub fn probe(&self, provider: &str) -> CredentialState {
        probe_provider_credentials(provider)
    }
}

pub trait CredentialProbe: Send + Sync {
    fn probe_provider(&self, provider: &str) -> CredentialState;
}

impl CredentialProbe for CredentialLedger {
    fn probe_provider(&self, provider: &str) -> CredentialState {
        self.probe(provider)
    }
}

/// Authoritative route for an interactive provider bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderRoute {
    Serving {
        model: String,
    },
    Fallback {
        selected: String,
        serving: String,
        reason: FallbackReason,
    },
    LoginPending {
        provider: String,
        since: SystemTime,
        prior: Box<ProviderRoute>,
    },
    Disconnected {
        selected: String,
        reason: DisconnectedReason,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteSnapshot {
    pub route: ProviderRoute,
    pub last_login_outcome: Option<LoginOutcome>,
    pub warning: Option<String>,
}

impl RouteSnapshot {
    pub fn serving_model(&self) -> Option<&str> {
        match &self.route {
            ProviderRoute::Serving { model } => Some(model),
            ProviderRoute::Fallback { serving, .. } => Some(serving),
            ProviderRoute::LoginPending { prior, .. } => serving_model_from_route(prior),
            ProviderRoute::Disconnected { .. } => None,
        }
    }
}

fn serving_model_from_route(route: &ProviderRoute) -> Option<&str> {
    match route {
        ProviderRoute::Serving { model } => Some(model),
        ProviderRoute::Fallback { serving, .. } => Some(serving),
        ProviderRoute::LoginPending { prior, .. } => serving_model_from_route(prior),
        ProviderRoute::Disconnected { .. } => None,
    }
}

struct RouteState {
    route: ProviderRoute,
    last_login_outcome: Option<LoginOutcome>,
    warning: Option<String>,
}

/// Owns the route state and bridge handle so they are updated together.
pub struct RouteController {
    state: RwLock<RouteState>,
    bridge: Arc<RwLock<Box<dyn LlmBridge>>>,
    events_tx: Option<broadcast::Sender<omegon_traits::AgentEvent>>,
}

impl RouteController {
    pub fn new(
        initial_route: ProviderRoute,
        initial_bridge: Box<dyn LlmBridge>,
        events_tx: Option<broadcast::Sender<omegon_traits::AgentEvent>>,
    ) -> Self {
        Self {
            state: RwLock::new(RouteState {
                route: initial_route,
                last_login_outcome: None,
                warning: None,
            }),
            bridge: Arc::new(RwLock::new(initial_bridge)),
            events_tx,
        }
    }

    pub fn bridge(&self) -> Arc<RwLock<Box<dyn LlmBridge>>> {
        self.bridge.clone()
    }

    pub async fn snapshot(&self) -> RouteSnapshot {
        let state = self.state.read().await;
        RouteSnapshot {
            route: state.route.clone(),
            last_login_outcome: state.last_login_outcome.clone(),
            warning: state.warning.clone(),
        }
    }

    pub async fn resolve_startup(
        selected_model: String,
        fallback_providers: &[String],
        ledger: &impl CredentialProbe,
    ) -> ProviderRoute {
        let selected_provider = crate::providers::infer_provider_id(&selected_model);
        match ledger.probe_provider(&selected_provider) {
            CredentialState::Valid { .. } => ProviderRoute::Serving {
                model: selected_model,
            },
            selected_state if fallback_providers.is_empty() => ProviderRoute::Disconnected {
                selected: selected_model,
                reason: disconnected_for_provider_state(selected_provider, selected_state),
            },
            selected_state => {
                let mut attempts = Vec::new();
                for provider in fallback_providers {
                    let state = ledger.probe_provider(provider);
                    if state.is_valid()
                        && let Some(serving) = crate::providers::default_model_for_provider(provider)
                    {
                        return ProviderRoute::Fallback {
                            selected: selected_model,
                            serving,
                            reason: fallback_reason_for_state(selected_provider, selected_state),
                        };
                    }
                    attempts.push(ProviderAttempt {
                        provider: provider.clone(),
                        state,
                    });
                }
                ProviderRoute::Disconnected {
                    selected: selected_model.clone(),
                    reason: DisconnectedReason::FallbackExhausted {
                        selected: selected_model,
                        attempts,
                    },
                }
            }
        }
    }

    pub async fn begin_login(&self, provider: String) -> RouteSnapshot {
        let mut state = self.state.write().await;
        let prior = Box::new(state.route.clone());
        state.route = ProviderRoute::LoginPending {
            provider,
            since: SystemTime::now(),
            prior,
        };
        state.warning = None;
        drop(state);
        self.emit_changed().await
    }

    pub async fn complete_login(
        &self,
        outcome: LoginOutcome,
        new_bridge: Option<Box<dyn LlmBridge>>,
    ) -> anyhow::Result<RouteSnapshot> {
        let mut state = self.state.write().await;
        let prior = match &state.route {
            ProviderRoute::LoginPending { prior, .. } => Some((**prior).clone()),
            _ => None,
        };

        match &outcome {
            LoginOutcome::Succeeded { model } => {
                let Some(bridge) = new_bridge else {
                    anyhow::bail!("login succeeded but no bridge was provided for {model}");
                };
                *self.bridge.write().await = bridge;
                state.route = ProviderRoute::Serving {
                    model: model.clone(),
                };
                state.warning = None;
            }
            LoginOutcome::Failed { reason } => {
                if let Some(prior) = prior {
                    state.route = prior;
                }
                state.warning = Some(login_failure_warning(reason));
            }
        }
        state.last_login_outcome = Some(outcome);
        drop(state);
        Ok(self.emit_changed().await)
    }

    pub async fn switch_model(
        &self,
        model: String,
        ledger: &impl CredentialProbe,
        new_bridge: Option<Box<dyn LlmBridge>>,
    ) -> anyhow::Result<RouteSnapshot> {
        let provider = crate::providers::infer_provider_id(&model);
        let credential_state = ledger.probe_provider(&provider);
        if !credential_state.is_valid() {
            let reason = disconnected_for_provider_state(provider, credential_state);
            let mut state = self.state.write().await;
            state.warning = Some(format!(
                "Model switch to {model} refused: {reason:?}"
            ));
            drop(state);
            return Ok(self.emit_changed().await);
        }

        let Some(bridge) = new_bridge else {
            let mut state = self.state.write().await;
            state.warning = Some(format!(
                "Model switch to {model} refused: credentials are valid but no bridge is available"
            ));
            drop(state);
            return Ok(self.emit_changed().await);
        };

        *self.bridge.write().await = bridge;
        let mut state = self.state.write().await;
        state.route = ProviderRoute::Serving { model };
        state.warning = None;
        drop(state);
        Ok(self.emit_changed().await)
    }

    pub async fn logout(&self, provider: String, selected_model: String) -> RouteSnapshot {
        let selected_provider = crate::providers::infer_provider_id(&selected_model);
        let mut state = self.state.write().await;
        if provider == selected_provider {
            state.route = ProviderRoute::Disconnected {
                selected: selected_model,
                reason: DisconnectedReason::MissingCredentials {
                    provider,
                    probed_sources: vec!["logout".to_string()],
                },
            };
            state.warning = Some("Logged out of the active provider; route is disconnected.".to_string());
        }
        drop(state);
        self.emit_changed().await
    }

    async fn emit_changed(&self) -> RouteSnapshot {
        let snapshot = self.snapshot().await;
        if let Some(tx) = &self.events_tx {
            let _ = tx.send(omegon_traits::AgentEvent::SystemNotification {
                message: route_summary(&snapshot),
            });
        }
        snapshot
    }
}

fn route_summary(snapshot: &RouteSnapshot) -> String {
    match &snapshot.route {
        ProviderRoute::Serving { model } => format!("Provider route: serving {model}"),
        ProviderRoute::Fallback {
            selected, serving, ..
        } => format!("Provider route: serving {serving} (fallback from {selected})"),
        ProviderRoute::LoginPending { provider, since, .. } => {
            let elapsed = since.elapsed().unwrap_or(Duration::ZERO).as_secs();
            format!("Provider login pending for {provider} ({elapsed}s)")
        }
        ProviderRoute::Disconnected { selected, reason } => {
            format!("Provider route disconnected for {selected}: {reason:?}")
        }
    }
}

fn login_failure_warning(reason: &LoginFailureReason) -> String {
    match reason {
        LoginFailureReason::Timeout => {
            "Login timed out. Close stale login tabs and run /login again.".to_string()
        }
        LoginFailureReason::StaleStateOnly => {
            "Login saw only stale callback tabs. Close old login tabs and run /login again."
                .to_string()
        }
        LoginFailureReason::Refused(detail) => format!("Login failed: {detail}"),
    }
}

fn disconnected_for_provider_state(provider: String, state: CredentialState) -> DisconnectedReason {
    match state {
        CredentialState::Valid { .. } => DisconnectedReason::ProviderUnavailable {
            provider,
            detail: "provider credentials are valid but no bridge is available".to_string(),
        },
        CredentialState::Expired {
            refreshable, ..
        } => DisconnectedReason::ExpiredCredentials {
            provider,
            refreshable,
        },
        CredentialState::Missing { probed_sources } => DisconnectedReason::MissingCredentials {
            provider,
            probed_sources,
        },
    }
}

fn fallback_reason_for_state(provider: String, state: CredentialState) -> FallbackReason {
    match state {
        CredentialState::Expired { .. } => FallbackReason::ExpiredCredentials { provider },
        CredentialState::Missing { .. } => FallbackReason::MissingCredentials { provider },
        CredentialState::Valid { .. } => FallbackReason::ProviderUnavailable {
            provider,
            detail: "selected provider had credentials but no usable bridge".to_string(),
        },
    }
}

fn probe_provider_credentials(provider: &str) -> CredentialState {
    let auth_key = crate::auth::auth_json_key(provider);
    let mut probed_sources = vec!["environment".to_string(), "auth.json".to_string()];

    for key in crate::auth::provider_env_vars(provider) {
        if std::env::var(key).ok().is_some_and(|v| !v.is_empty()) {
            return CredentialState::Valid {
                source: CredentialSource::Environment,
                oauth: false,
            };
        }
    }

    if let Some(creds) = crate::auth::read_credentials(auth_key) {
        if creds.cred_type == "oauth" && creds.is_expired() {
            return CredentialState::Expired {
                source: CredentialSource::AuthJson,
                refreshable: !creds.refresh.is_empty(),
            };
        }
        return CredentialState::Valid {
            source: CredentialSource::AuthJson,
            oauth: creds.cred_type == "oauth",
        };
    }

    probed_sources.push("external".to_string());
    if let Some(creds) = crate::auth::read_external_credentials(auth_key) {
        if creds.cred_type == "oauth" && creds.is_expired() {
            return CredentialState::Expired {
                source: CredentialSource::External,
                refreshable: !creds.refresh.is_empty(),
            };
        }
        return CredentialState::Valid {
            source: CredentialSource::External,
            oauth: creds.cred_type == "oauth",
        };
    }

    CredentialState::Missing { probed_sources }
}

impl Default for RouteController {
    fn default() -> Self {
        Self::new(
            ProviderRoute::Disconnected {
                selected: String::new(),
                reason: DisconnectedReason::ProviderUnavailable {
                    provider: String::new(),
                    detail: "route controller not initialized".to_string(),
                },
            },
            Box::new(NullBridge),
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct StubLedger(HashMap<String, CredentialState>);

    impl CredentialProbe for StubLedger {
        fn probe_provider(&self, provider: &str) -> CredentialState {
            self.0
                .get(provider)
                .cloned()
                .unwrap_or(CredentialState::Missing {
                    probed_sources: vec!["stub".to_string()],
                })
        }
    }

    fn valid() -> CredentialState {
        CredentialState::Valid {
            source: CredentialSource::AuthJson,
            oauth: true,
        }
    }

    fn expired() -> CredentialState {
        CredentialState::Expired {
            source: CredentialSource::AuthJson,
            refreshable: true,
        }
    }

    fn missing() -> CredentialState {
        CredentialState::Missing {
            probed_sources: vec!["stub".to_string()],
        }
    }

    fn ledger(entries: &[(&str, CredentialState)]) -> StubLedger {
        StubLedger(
            entries
                .iter()
                .map(|(provider, state)| ((*provider).to_string(), state.clone()))
                .collect(),
        )
    }

    #[tokio::test]
    async fn begin_login_preserves_prior_route() {
        let controller = RouteController::new(
            ProviderRoute::Serving {
                model: "anthropic:claude-fable-5".into(),
            },
            Box::new(NullBridge),
            None,
        );
        let snapshot = controller.begin_login("openai-codex".into()).await;
        match snapshot.route {
            ProviderRoute::LoginPending { provider, prior, .. } => {
                assert_eq!(provider, "openai-codex");
                assert_eq!(
                    *prior,
                    ProviderRoute::Serving {
                        model: "anthropic:claude-fable-5".into()
                    }
                );
            }
            other => panic!("expected LoginPending, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn failed_login_reverts_to_prior_and_sets_warning() {
        let controller = RouteController::new(
            ProviderRoute::Fallback {
                selected: "openai-codex:gpt-5.5".into(),
                serving: "anthropic:claude-fable-5".into(),
                reason: FallbackReason::MissingCredentials {
                    provider: "openai-codex".into(),
                },
            },
            Box::new(NullBridge),
            None,
        );
        controller.begin_login("openai-codex".into()).await;
        let snapshot = controller
            .complete_login(
                LoginOutcome::Failed {
                    reason: LoginFailureReason::StaleStateOnly,
                },
                None,
            )
            .await
            .unwrap();
        assert!(matches!(snapshot.route, ProviderRoute::Fallback { .. }));
        assert!(snapshot.warning.unwrap().contains("stale callback"));
    }

    #[tokio::test]
    async fn successful_login_installs_serving_route() {
        let controller = RouteController::default();
        controller.begin_login("anthropic".into()).await;
        let snapshot = controller
            .complete_login(
                LoginOutcome::Succeeded {
                    model: "anthropic:claude-fable-5".into(),
                },
                Some(Box::new(NullBridge)),
            )
            .await
            .unwrap();
        assert_eq!(
            snapshot.route,
            ProviderRoute::Serving {
                model: "anthropic:claude-fable-5".into()
            }
        );
        assert!(snapshot.warning.is_none());
    }

    #[tokio::test]
    async fn switch_model_refuses_missing_credentials_without_changing_route() {
        let controller = RouteController::new(
            ProviderRoute::Serving {
                model: "anthropic:claude-fable-5".into(),
            },
            Box::new(NullBridge),
            None,
        );
        let snapshot = controller
            .switch_model(
                "openai-codex:gpt-5.5".into(),
                &ledger(&[("openai-codex", missing())]),
                Some(Box::new(NullBridge)),
            )
            .await
            .unwrap();
        assert_eq!(
            snapshot.route,
            ProviderRoute::Serving {
                model: "anthropic:claude-fable-5".into()
            }
        );
        assert!(snapshot.warning.unwrap().contains("refused"));
    }

    #[tokio::test]
    async fn switch_model_with_valid_credentials_installs_serving_route() {
        let controller = RouteController::default();
        let snapshot = controller
            .switch_model(
                "openai-codex:gpt-5.5".into(),
                &ledger(&[("openai-codex", valid())]),
                Some(Box::new(NullBridge)),
            )
            .await
            .unwrap();
        assert_eq!(
            snapshot.route,
            ProviderRoute::Serving {
                model: "openai-codex:gpt-5.5".into()
            }
        );
        assert!(snapshot.warning.is_none());
    }

    #[tokio::test]
    async fn logout_active_provider_disconnects_route() {
        let controller = RouteController::new(
            ProviderRoute::Serving {
                model: "anthropic:claude-fable-5".into(),
            },
            Box::new(NullBridge),
            None,
        );
        let snapshot = controller
            .logout("anthropic".into(), "anthropic:claude-fable-5".into())
            .await;
        assert!(matches!(snapshot.route, ProviderRoute::Disconnected { .. }));
        assert!(snapshot.warning.unwrap().contains("disconnected"));
    }

    #[tokio::test]
    async fn startup_matrix_is_total_and_fallback_is_explicit() {
        let selected_states = [valid(), expired(), missing()];
        let fallback_sets: Vec<Vec<String>> = vec![
            vec![],
            vec!["anthropic".to_string()],
            vec!["google".to_string()],
        ];

        for selected_state in selected_states {
            for fallback_providers in &fallback_sets {
                let ledger = ledger(&[
                    ("openai-codex", selected_state.clone()),
                    ("anthropic", valid()),
                    ("google", missing()),
                ]);
                let route = RouteController::resolve_startup(
                    "openai-codex:gpt-5.5".into(),
                    fallback_providers,
                    &ledger,
                )
                .await;

                match (&selected_state, fallback_providers.as_slice(), route) {
                    (CredentialState::Valid { .. }, _, ProviderRoute::Serving { .. }) => {}
                    (_, [], ProviderRoute::Disconnected { .. }) => {}
                    (_, [provider], ProviderRoute::Fallback { serving, .. })
                        if provider == "anthropic" =>
                    {
                        assert!(serving.starts_with("anthropic:"));
                    }
                    (_, [provider], ProviderRoute::Disconnected { reason, .. })
                        if provider == "google" =>
                    {
                        assert!(matches!(reason, DisconnectedReason::FallbackExhausted { .. }));
                    }
                    other => panic!("unexpected startup route matrix result: {other:?}"),
                }
            }
        }
    }
}

