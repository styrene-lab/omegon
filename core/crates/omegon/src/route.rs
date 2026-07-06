//! Provider route state machine.
//!
//! This module is the authoritative model for "what provider/model is serving
//! this interactive session?". It intentionally contains no TUI types; UI,
//! loop, web, and daemon-facing surfaces consume [`RouteSnapshot`] instead.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::{RwLock, broadcast};

use crate::bridge::{LlmBridge, NullBridge};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelGrade {
    F,
    D,
    C,
    B,
    A,
    S,
}

impl ModelGrade {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "F" => Some(Self::F),
            "D" => Some(Self::D),
            "C" => Some(Self::C),
            "B" => Some(Self::B),
            "A" => Some(Self::A),
            "S" => Some(Self::S),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::F => "F",
            Self::D => "D",
            Self::C => "C",
            Self::B => "B",
            Self::A => "A",
            Self::S => "S",
        }
    }

    pub fn to_capability_grade_band(&self) -> crate::routing::CapabilityGradeBand {
        match self {
            Self::S => crate::routing::CapabilityGradeBand::Max,
            Self::A | Self::B => crate::routing::CapabilityGradeBand::Frontier,
            Self::C | Self::D => crate::routing::CapabilityGradeBand::Mid,
            Self::F => crate::routing::CapabilityGradeBand::Leaf,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderSelection {
    Auto,
    Local,
    Upstream,
    Endpoint(String),
}

impl ProviderSelection {
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        match trimmed {
            "auto" => Some(Self::Auto),
            "local" => Some(Self::Local),
            "upstream" => Some(Self::Upstream),
            "" => None,
            other => Some(Self::Endpoint(other.to_string())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GradePolicy {
    Exact,
    Minimum,
    NearestAllowed { max_downgrade_steps: u8 },
}

impl GradePolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "exact" => Some(Self::Exact),
            "minimum" => Some(Self::Minimum),
            "nearest" => Some(Self::NearestAllowed {
                max_downgrade_steps: 1,
            }),
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Minimum => "minimum",
            Self::NearestAllowed { .. } => "nearest",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailoverPolicy {
    SameGradeOtherEndpoint,
    AnyPolicyCompliantEndpoint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DegradationPolicy {
    None,
    OneStep,
    BestEffort,
    Ask,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelIntent {
    pub grade: Option<ModelGrade>,
    pub provider_selection: ProviderSelection,
    pub grade_policy: GradePolicy,
    pub failover_policy: FailoverPolicy,
    pub degradation_policy: DegradationPolicy,
    pub provider_policy: Option<crate::semantic_route::ProviderPolicy>,
    pub exact_model_override: Option<String>,
}

impl Default for ModelIntent {
    fn default() -> Self {
        Self {
            grade: Some(ModelGrade::B),
            provider_selection: ProviderSelection::Auto,
            grade_policy: GradePolicy::Minimum,
            failover_policy: FailoverPolicy::AnyPolicyCompliantEndpoint,
            degradation_policy: DegradationPolicy::Ask,
            provider_policy: None,
            exact_model_override: None,
        }
    }
}

impl ModelIntent {
    pub fn pinned_model(model: String) -> Self {
        Self {
            exact_model_override: Some(model),
            ..Self::default()
        }
    }

    pub fn with_grade(grade: ModelGrade) -> Self {
        Self {
            grade: Some(grade),
            exact_model_override: None,
            ..Self::default()
        }
    }

    pub fn to_capability_request(&self) -> crate::routing::CapabilityRequest {
        let grade = if let Some(exact) = self.exact_model_override.as_deref() {
            crate::routing::infer_model_grade_band(exact)
        } else {
            self.grade
                .clone()
                .unwrap_or(ModelGrade::B)
                .to_capability_grade_band()
        };
        let mut req = crate::routing::CapabilityRequest {
            grade,
            ..Default::default()
        };
        match &self.provider_selection {
            ProviderSelection::Auto => {}
            ProviderSelection::Local => {
                req.only_providers.push("ollama".into());
                req.prefer_local = true;
            }
            ProviderSelection::Upstream => req.avoid_providers.push("ollama".into()),
            ProviderSelection::Endpoint(endpoint) => req.only_providers.push(endpoint.clone()),
        }
        if let Some(exact) = self.exact_model_override.as_deref() {
            let provider = crate::providers::infer_provider_id(exact);
            req.only_providers.clear();
            req.only_providers.push(provider);
        }
        req
    }

    pub fn to_provider_policy(&self) -> crate::semantic_route::ProviderPolicy {
        if let Some(policy) = self.provider_policy {
            return policy;
        }
        match &self.provider_selection {
            ProviderSelection::Auto => crate::semantic_route::ProviderPolicy::Auto,
            ProviderSelection::Local => crate::semantic_route::ProviderPolicy::LocalOnly,
            ProviderSelection::Upstream => crate::semantic_route::ProviderPolicy::Auto,
            ProviderSelection::Endpoint(endpoint) if endpoint == "github-copilot" => {
                crate::semantic_route::ProviderPolicy::CopilotOnly
            }
            ProviderSelection::Endpoint(_) => crate::semantic_route::ProviderPolicy::Auto,
        }
    }

    pub fn summary(&self) -> String {
        if let Some(model) = &self.exact_model_override {
            return format!("pinned {model}");
        }
        let grade = self
            .grade
            .as_ref()
            .map(ModelGrade::as_str)
            .unwrap_or("auto");
        let provider = match &self.provider_selection {
            ProviderSelection::Auto => "auto".to_string(),
            ProviderSelection::Local => "local".to_string(),
            ProviderSelection::Upstream => "upstream".to_string(),
            ProviderSelection::Endpoint(endpoint) => endpoint.clone(),
        };
        format!(
            "grade {grade}, provider {provider}, policy {}",
            self.grade_policy.label()
        )
    }
}

/// Select the highest-ranked provider/model candidate for a durable model intent
/// without mutating the active route or erasing the intent.
pub fn select_candidate_for_intent(
    intent: &ModelIntent,
    inventory: &crate::routing::ProviderInventory,
) -> Option<crate::routing::ProviderCandidate> {
    let req = intent.to_capability_request();
    crate::routing::route(&req, inventory).into_iter().next()
}

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
    Valid {
        source: CredentialSource,
        oauth: bool,
    },
    Expired {
        source: CredentialSource,
        refreshable: bool,
    },
    Unreadable {
        source: CredentialSource,
        detail: String,
    },
    Missing {
        probed_sources: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialSource {
    Environment,
    AuthJson,
    External,
}

impl CredentialSource {
    pub fn label(&self) -> &'static str {
        match self {
            CredentialSource::Environment => "environment",
            CredentialSource::AuthJson => "auth.json",
            CredentialSource::External => "external",
        }
    }
}

impl CredentialState {
    pub fn is_valid(&self) -> bool {
        matches!(self, CredentialState::Valid { .. })
    }

    pub fn summary(&self) -> String {
        match self {
            CredentialState::Valid { source, oauth } => {
                let kind = if *oauth { "OAuth" } else { "API key" };
                format!("valid {kind} credentials from {}", source.label())
            }
            CredentialState::Expired {
                source,
                refreshable,
            } => {
                let refresh = if *refreshable {
                    "refreshable"
                } else {
                    "not refreshable"
                };
                format!(
                    "expired OAuth credentials from {} ({refresh})",
                    source.label()
                )
            }
            CredentialState::Unreadable { source, detail } => {
                format!("unreadable credentials from {}: {detail}", source.label())
            }
            CredentialState::Missing { probed_sources } => {
                format!("missing credentials; probed {}", probed_sources.join(", "))
            }
        }
    }
}

impl DisconnectedReason {
    pub fn operator_message(&self, selected: &str) -> String {
        match self {
            DisconnectedReason::MissingCredentials {
                provider,
                probed_sources,
            } => format!(
                "No credentials for selected model {selected} ({provider}). Probed: {}. Remediation: run `/login {provider}` or set one of: {}.",
                probed_sources.join(", "),
                provider_env_var_list(provider)
            ),
            DisconnectedReason::ExpiredCredentials {
                provider,
                refreshable,
            } => {
                let refresh = if *refreshable {
                    "refreshable token expired"
                } else {
                    "token expired and is not refreshable"
                };
                format!(
                    "Expired credentials for selected model {selected} ({provider}): {refresh}. Remediation: run `/login {provider}` or set one of: {}.",
                    provider_env_var_list(provider)
                )
            }
            DisconnectedReason::FallbackExhausted { selected, attempts } => {
                let tried = attempts
                    .iter()
                    .map(|attempt| format!("{}: {}", attempt.provider, attempt.state.summary()))
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(
                    "No usable route for selected model {selected}. Explicit fallbackProviders exhausted: {tried}. Remediation: run `/login {}` or configure fallbackProviders with a provider that has credentials.",
                    crate::providers::infer_provider_id(selected)
                )
            }
            DisconnectedReason::ProviderUnavailable { provider, detail } => {
                let remediation = if detail.contains("credential") {
                    "check or remove the unreadable provider entry from auth.json, then run `/login ".to_string()
                        + provider
                        + "` if the entry cannot be repaired"
                } else {
                    "run `/login ".to_string() + provider + "` or check provider configuration"
                };
                format!(
                    "Provider {provider} is unavailable for selected model {selected}: {detail}. Remediation: {remediation}."
                )
            }
        }
    }
}

fn provider_env_var_list(provider: &str) -> String {
    let vars = crate::auth::provider_env_vars(provider);
    if vars.is_empty() {
        format!(
            "{}_API_KEY",
            provider.to_ascii_uppercase().replace('-', "_")
        )
    } else {
        vars.join(", ")
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
    pub intent: ModelIntent,
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

    pub fn operator_status(&self) -> String {
        let mut lines = vec![format!("Provider route: {}", route_summary(self))];
        lines.push(format!("Model intent: {}", self.intent.summary()));
        if let Some(warning) = &self.warning {
            lines.push(format!("Route warning: {warning}"));
        }
        if let Some(outcome) = &self.last_login_outcome {
            lines.push(format!(
                "Last login outcome: {}",
                login_outcome_summary(outcome)
            ));
        }
        lines.join("\n")
    }
}

pub(crate) fn intent_from_route(route: &ProviderRoute) -> ModelIntent {
    match route {
        ProviderRoute::Serving { model } => ModelIntent::pinned_model(model.clone()),
        ProviderRoute::Fallback { selected, .. } | ProviderRoute::Disconnected { selected, .. } => {
            ModelIntent::pinned_model(selected.clone())
        }
        ProviderRoute::LoginPending { prior, .. } => intent_from_route(prior),
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
    intent: ModelIntent,
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
        let intent = intent_from_route(&initial_route);
        Self::with_initial_intent(initial_route, initial_bridge, events_tx, intent)
    }

    pub fn with_initial_intent(
        initial_route: ProviderRoute,
        initial_bridge: Box<dyn LlmBridge>,
        events_tx: Option<broadcast::Sender<omegon_traits::AgentEvent>>,
        intent: ModelIntent,
    ) -> Self {
        Self {
            state: RwLock::new(RouteState {
                intent,
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
            intent: state.intent.clone(),
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
        crate::auth::trace_auth_store_probe(&selected_provider, "route_startup:selected");
        let selected_probe = ledger.probe_provider(&selected_provider);
        tracing::info!(selected_model = %selected_model, selected_provider = %selected_provider, credential_state = %selected_probe.summary(), fallback_providers = ?fallback_providers, "provider route startup credential probe");
        match selected_probe {
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
                    crate::auth::trace_auth_store_probe(provider, "route_startup:fallback");
                    let state = ledger.probe_provider(provider);
                    tracing::info!(selected_model = %selected_model, fallback_provider = %provider, credential_state = %state.summary(), "provider route fallback credential probe");
                    if state.is_valid()
                        && let Some(serving) =
                            crate::providers::default_model_for_provider(provider)
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
        tracing::info!(provider = %provider, prior_route = ?state.route, "provider route login started");
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

        tracing::info!(outcome = ?outcome, prior_route = ?prior, "provider route login completed");
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

    pub async fn set_model_intent(&self, intent: ModelIntent) -> RouteSnapshot {
        let mut state = self.state.write().await;
        state.intent = intent;
        state.warning = None;
        drop(state);
        self.emit_changed().await
    }

    pub async fn clear_exact_model_override(&self) -> RouteSnapshot {
        let mut state = self.state.write().await;
        state.intent.exact_model_override = None;
        if state.intent.grade.is_none() {
            state.intent.grade = Some(ModelGrade::B);
        }
        state.warning = None;
        drop(state);
        self.emit_changed().await
    }

    pub async fn set_provider_selection(
        &self,
        provider_selection: ProviderSelection,
    ) -> RouteSnapshot {
        let mut state = self.state.write().await;
        state.intent.provider_selection = provider_selection;
        state.intent.exact_model_override = None;
        if state.intent.grade.is_none() {
            state.intent.grade = Some(ModelGrade::B);
        }
        state.warning = None;
        drop(state);
        self.emit_changed().await
    }

    pub async fn set_provider_policy(
        &self,
        provider_policy: Option<crate::semantic_route::ProviderPolicy>,
    ) -> RouteSnapshot {
        let mut state = self.state.write().await;
        state.intent.provider_policy = provider_policy;
        state.intent.exact_model_override = None;
        if state.intent.grade.is_none() {
            state.intent.grade = Some(ModelGrade::B);
        }
        state.warning = None;
        drop(state);
        self.emit_changed().await
    }

    pub async fn set_grade_policy(&self, grade_policy: GradePolicy) -> RouteSnapshot {
        let mut state = self.state.write().await;
        state.intent.grade_policy = grade_policy;
        state.intent.exact_model_override = None;
        if state.intent.grade.is_none() {
            state.intent.grade = Some(ModelGrade::B);
        }
        state.warning = None;
        drop(state);
        self.emit_changed().await
    }

    pub async fn resolve_route_from_intent_candidate(
        &self,
        candidate: crate::routing::ProviderCandidate,
        new_bridge: Box<dyn LlmBridge>,
    ) -> anyhow::Result<RouteSnapshot> {
        let serving = format!("{}:{}", candidate.provider_id, candidate.model_id);
        *self.bridge.write().await = new_bridge;
        let mut state = self.state.write().await;
        state.route = ProviderRoute::Serving { model: serving };
        state.warning = None;
        drop(state);
        Ok(self.emit_changed().await)
    }

    pub async fn resolve_route_from_intent_inventory(
        &self,
        inventory: &crate::routing::ProviderInventory,
        new_bridge: Box<dyn LlmBridge>,
    ) -> anyhow::Result<RouteSnapshot> {
        let intent = {
            let state = self.state.read().await;
            state.intent.clone()
        };
        let Some(candidate) = select_candidate_for_intent(&intent, inventory) else {
            let mut state = self.state.write().await;
            state.warning = Some(format!(
                "No provider candidate satisfies model intent: {}",
                intent.summary()
            ));
            drop(state);
            return Ok(self.emit_changed().await);
        };
        self.resolve_route_from_intent_candidate(candidate, new_bridge)
            .await
    }

    pub async fn switch_model(
        &self,
        model: String,
        ledger: &impl CredentialProbe,
        new_bridge: Option<Box<dyn LlmBridge>>,
    ) -> anyhow::Result<RouteSnapshot> {
        let provider = crate::providers::infer_provider_id(&model);
        crate::auth::trace_auth_store_probe(&provider, "route_switch_model");
        let credential_state = ledger.probe_provider(&provider);
        tracing::info!(model = %model, provider = %provider, credential_state = %credential_state.summary(), "provider route model switch credential probe");
        if !credential_state.is_valid() {
            let reason = disconnected_for_provider_state(provider, credential_state);
            let mut state = self.state.write().await;
            state.warning = Some(format!("Model switch to {model} refused: {reason:?}"));
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
        state.intent = ModelIntent::pinned_model(model.clone());
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
            state.warning =
                Some("Logged out of the active provider; route is disconnected.".to_string());
        }
        drop(state);
        self.emit_changed().await
    }

    async fn emit_changed(&self) -> RouteSnapshot {
        let snapshot = self.snapshot().await;
        if let Some(tx) = &self.events_tx {
            let (state, selected, serving) = route_event_fields(&snapshot.route);
            let _ = tx.send(omegon_traits::AgentEvent::RouteChanged {
                state,
                selected,
                serving,
                warning: snapshot.warning.clone(),
                message: route_summary(&snapshot),
            });
        }
        snapshot
    }
}

fn route_event_fields(route: &ProviderRoute) -> (String, Option<String>, Option<String>) {
    match route {
        ProviderRoute::Serving { model } => {
            ("serving".into(), Some(model.clone()), Some(model.clone()))
        }
        ProviderRoute::Fallback {
            selected, serving, ..
        } => (
            "fallback".into(),
            Some(selected.clone()),
            Some(serving.clone()),
        ),
        ProviderRoute::LoginPending {
            provider, prior, ..
        } => {
            let selected = route_event_fields(prior).1;
            ("login_pending".into(), selected, Some(provider.clone()))
        }
        ProviderRoute::Disconnected { selected, .. } => {
            ("disconnected".into(), Some(selected.clone()), None)
        }
    }
}

fn login_outcome_summary(outcome: &LoginOutcome) -> String {
    match outcome {
        LoginOutcome::Succeeded { model } => format!("succeeded; serving {model}"),
        LoginOutcome::Failed { reason } => format!("failed; {}", login_failure_label(reason)),
    }
}

fn route_summary(snapshot: &RouteSnapshot) -> String {
    match &snapshot.route {
        ProviderRoute::Serving { model } => format!("Provider route: serving {model}"),
        ProviderRoute::Fallback {
            selected, serving, ..
        } => format!("Provider route: serving {serving} (fallback from {selected})"),
        ProviderRoute::LoginPending {
            provider, since, ..
        } => {
            let elapsed = since.elapsed().unwrap_or(Duration::ZERO).as_secs();
            format!("Provider login pending for {provider} ({elapsed}s)")
        }
        ProviderRoute::Disconnected { selected, reason } => {
            format!("Provider route disconnected for {selected}: {reason:?}")
        }
    }
}

fn login_failure_label(reason: &LoginFailureReason) -> String {
    match reason {
        LoginFailureReason::Timeout => "timed out".to_string(),
        LoginFailureReason::StaleStateOnly => "only stale callback tabs were observed".to_string(),
        LoginFailureReason::Refused(detail) => format!("refused: {detail}"),
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
        CredentialState::Expired { refreshable, .. } => DisconnectedReason::ExpiredCredentials {
            provider,
            refreshable,
        },
        CredentialState::Unreadable { source, detail } => DisconnectedReason::ProviderUnavailable {
            provider,
            detail: format!(
                "{} credential entry is unreadable: {detail}",
                source.label()
            ),
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
        CredentialState::Unreadable { source, detail } => FallbackReason::ProviderUnavailable {
            provider,
            detail: format!(
                "{} credential entry is unreadable: {detail}",
                source.label()
            ),
        },
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

    crate::auth::trace_auth_store_probe(auth_key, "route_credential_ledger");

    if let Some(path) = crate::auth::auth_json_path()
        && let Ok(content) = std::fs::read_to_string(&path)
    {
        match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(auth) => {
                if let Some(entry) = auth.get(auth_key) {
                    match serde_json::from_value::<crate::auth::OAuthCredentials>(entry.clone()) {
                        Ok(creds) => {
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
                        Err(error) => {
                            return CredentialState::Unreadable {
                                source: CredentialSource::AuthJson,
                                detail: error.to_string(),
                            };
                        }
                    }
                }
            }
            Err(error) => {
                return CredentialState::Unreadable {
                    source: CredentialSource::AuthJson,
                    detail: format!("{}: {error}", path.display()),
                };
            }
        }
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
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn unreadable() -> CredentialState {
        CredentialState::Unreadable {
            source: CredentialSource::AuthJson,
            detail: "invalid provider entry".to_string(),
        }
    }

    fn temp_auth_path(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("omegon-{label}-{nanos}-auth.json"))
    }

    fn with_auth_path<T>(path: &Path, f: impl FnOnce() -> T + std::panic::UnwindSafe) -> T {
        let _guard = crate::auth::TEST_AUTH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let original = std::env::var("OMEGON_AUTH_JSON_PATH").ok();
        // SAFETY: Tests serialize auth environment mutations with TEST_AUTH_ENV_LOCK,
        // and restore the original value before releasing the lock.
        unsafe { std::env::set_var("OMEGON_AUTH_JSON_PATH", path) };
        let result = std::panic::catch_unwind(f);
        match original {
            Some(value) => {
                // SAFETY: Protected by TEST_AUTH_ENV_LOCK as above.
                unsafe { std::env::set_var("OMEGON_AUTH_JSON_PATH", value) };
            }
            None => {
                // SAFETY: Protected by TEST_AUTH_ENV_LOCK as above.
                unsafe { std::env::remove_var("OMEGON_AUTH_JSON_PATH") };
            }
        }
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
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
            ProviderRoute::LoginPending {
                provider, prior, ..
            } => {
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
    async fn timeout_login_reverts_to_prior_fallback_with_retry_guidance() {
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
                    reason: LoginFailureReason::Timeout,
                },
                None,
            )
            .await
            .unwrap();
        assert!(matches!(snapshot.route, ProviderRoute::Fallback { .. }));
        let warning = snapshot.warning.unwrap();
        assert!(warning.contains("timed out"), "{warning}");
        assert!(warning.contains("run /login again"), "{warning}");
    }

    #[tokio::test]
    async fn successful_login_from_fallback_clears_warning_and_serves_selected_model() {
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
                LoginOutcome::Succeeded {
                    model: "openai-codex:gpt-5.5".into(),
                },
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

    #[test]
    fn disconnected_message_lists_sources_and_remediation() {
        let reason = DisconnectedReason::MissingCredentials {
            provider: "openai-codex".into(),
            probed_sources: vec!["environment".into(), "auth.json".into(), "external".into()],
        };
        let message = reason.operator_message("openai-codex:gpt-5.5");
        assert!(message.contains("openai-codex:gpt-5.5"), "{message}");
        assert!(
            message.contains("environment, auth.json, external"),
            "{message}"
        );
        assert!(message.contains("/login openai-codex"), "{message}");
        assert!(message.contains("CHATGPT_OAUTH_TOKEN"), "{message}");
    }

    #[test]
    fn fallback_exhausted_message_lists_each_attempt() {
        let reason = DisconnectedReason::FallbackExhausted {
            selected: "openai-codex:gpt-5.5".into(),
            attempts: vec![
                ProviderAttempt {
                    provider: "anthropic".into(),
                    state: expired(),
                },
                ProviderAttempt {
                    provider: "google".into(),
                    state: missing(),
                },
            ],
        };
        let message = reason.operator_message("openai-codex:gpt-5.5");
        assert!(message.contains("fallbackProviders exhausted"), "{message}");
        assert!(message.contains("anthropic: expired OAuth"), "{message}");
        assert!(message.contains("google: missing credentials"), "{message}");
    }

    #[test]
    fn auth_json_bad_provider_entry_is_unreadable_not_missing() {
        let auth_path = temp_auth_path("bad-codex-entry");
        fs::write(
            &auth_path,
            r#"{"openai-codex":{"type":"oauth","access":"token","refresh":"refresh"}}"#,
        )
        .unwrap();

        let state = with_auth_path(&auth_path, || probe_provider_credentials("openai-codex"));
        let _ = fs::remove_file(&auth_path);

        match state {
            CredentialState::Unreadable {
                source: CredentialSource::AuthJson,
                detail,
            } => assert!(detail.contains("missing field `expires`"), "{detail}"),
            other => panic!("expected unreadable auth.json entry, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn startup_unreadable_selected_credentials_do_not_report_missing_login() {
        let route = RouteController::resolve_startup(
            "openai-codex:gpt-5.5".into(),
            &[],
            &ledger(&[("openai-codex", unreadable())]),
        )
        .await;

        match route {
            ProviderRoute::Disconnected { selected, reason } => {
                assert_eq!(selected, "openai-codex:gpt-5.5");
                let message = reason.operator_message(&selected);
                assert!(message.contains("unreadable"), "{message}");
                assert!(!message.contains("No credentials"), "{message}");
                assert!(message.contains("auth.json"), "{message}");
            }
            other => panic!("expected disconnected route, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn timeout_reverts_prior_fallback_and_warns_until_success() {
        let prior = ProviderRoute::Fallback {
            selected: "openai-codex:gpt-5.5".into(),
            serving: "anthropic:claude-fable-5".into(),
            reason: FallbackReason::MissingCredentials {
                provider: "openai-codex".into(),
            },
        };
        let controller = RouteController::new(prior.clone(), Box::new(NullBridge), None);
        controller.begin_login("openai-codex".into()).await;
        let failed = controller
            .complete_login(
                LoginOutcome::Failed {
                    reason: LoginFailureReason::Timeout,
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(failed.route, prior);
        assert!(failed.warning.unwrap().contains("timed out"));

        controller.begin_login("openai-codex".into()).await;
        let succeeded = controller
            .complete_login(
                LoginOutcome::Succeeded {
                    model: "openai-codex:gpt-5.5".into(),
                },
                Some(Box::new(NullBridge)),
            )
            .await
            .unwrap();
        assert_eq!(
            succeeded.route,
            ProviderRoute::Serving {
                model: "openai-codex:gpt-5.5".into()
            }
        );
        assert!(succeeded.warning.is_none());
    }

    #[tokio::test]
    async fn stale_state_only_failure_gives_close_old_tabs_guidance() {
        let controller = RouteController::default();
        controller.begin_login("openai-codex".into()).await;
        let failed = controller
            .complete_login(
                LoginOutcome::Failed {
                    reason: LoginFailureReason::StaleStateOnly,
                },
                None,
            )
            .await
            .unwrap();
        let warning = failed.warning.unwrap();
        assert!(warning.contains("stale callback tabs"), "{warning}");
        assert!(warning.contains("Close old login tabs"), "{warning}");
    }

    #[test]
    fn route_status_reports_login_pending_and_last_outcome() {
        let pending = RouteSnapshot {
            intent: ModelIntent::default(),
            route: ProviderRoute::LoginPending {
                provider: "openai-codex".into(),
                since: SystemTime::now(),
                prior: Box::new(ProviderRoute::Fallback {
                    selected: "openai-codex:gpt-5.5".into(),
                    serving: "anthropic:claude-fable-5".into(),
                    reason: FallbackReason::MissingCredentials {
                        provider: "openai-codex".into(),
                    },
                }),
            },
            last_login_outcome: Some(LoginOutcome::Failed {
                reason: LoginFailureReason::Timeout,
            }),
            warning: Some("Login failed: timed out".into()),
        };
        let status = pending.operator_status();
        assert!(
            status.contains("Provider login pending for openai-codex"),
            "{status}"
        );
        assert!(
            status.contains("Last login outcome: failed; timed out"),
            "{status}"
        );
        assert!(status.contains("Route warning: Login failed"), "{status}");
    }

    #[tokio::test]
    async fn empty_fallback_startup_disconnects_and_null_bridge_errors() {
        let route = RouteController::resolve_startup(
            "openai-codex:gpt-5.5".into(),
            &[],
            &ledger(&[("openai-codex", missing())]),
        )
        .await;
        assert!(matches!(route, ProviderRoute::Disconnected { .. }));

        let controller = RouteController::new(route, Box::new(NullBridge), None);
        let bridge = controller.bridge();
        let stream_result = bridge
            .read()
            .await
            .stream("", &[], &[], &crate::bridge::StreamOptions::default())
            .await;
        let err = stream_result.unwrap_err().to_string();
        assert!(err.contains("No LLM provider configured"), "{err}");
    }

    #[tokio::test]
    async fn configured_fallback_startup_emits_fallback_route_summary() {
        let route = RouteController::resolve_startup(
            "openai-codex:gpt-5.5".into(),
            &["anthropic".to_string()],
            &ledger(&[("openai-codex", missing()), ("anthropic", valid())]),
        )
        .await;
        assert!(matches!(route, ProviderRoute::Fallback { .. }));

        let (tx, mut rx) = broadcast::channel(4);
        let controller = RouteController::new(route, Box::new(NullBridge), Some(tx));
        controller.begin_login("openai-codex".into()).await;
        let event = rx.recv().await.unwrap();
        let omegon_traits::AgentEvent::RouteChanged {
            state,
            serving,
            message,
            ..
        } = event
        else {
            panic!("route controller should emit a first-class RouteChanged event");
        };
        assert_eq!(state, "login_pending");
        assert_eq!(serving.as_deref(), Some("openai-codex"));
        assert!(
            message.contains("Provider login pending for openai-codex"),
            "{message}"
        );
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
                        assert!(matches!(
                            reason,
                            DisconnectedReason::FallbackExhausted { .. }
                        ));
                    }
                    other => panic!("unexpected startup route matrix result: {other:?}"),
                }
            }
        }
    }
    #[tokio::test]
    async fn model_intent_grade_update_preserves_serving_route() {
        let controller = RouteController::new(
            ProviderRoute::Serving {
                model: "anthropic:claude-sonnet-4-6".into(),
            },
            Box::new(NullBridge),
            None,
        );

        let snapshot = controller
            .set_model_intent(ModelIntent::with_grade(ModelGrade::S))
            .await;

        assert_eq!(
            snapshot.serving_model(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(snapshot.intent.grade, Some(ModelGrade::S));
        assert_eq!(snapshot.intent.exact_model_override, None);
    }
    #[tokio::test]
    async fn clear_exact_override_preserves_serving_route() {
        let controller = RouteController::new(
            ProviderRoute::Serving {
                model: "anthropic:claude-sonnet-4-6".into(),
            },
            Box::new(NullBridge),
            None,
        );

        let snapshot = controller.clear_exact_model_override().await;

        assert_eq!(
            snapshot.serving_model(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(snapshot.intent.exact_model_override, None);
        assert_eq!(snapshot.intent.grade, Some(ModelGrade::B));
    }
    #[tokio::test]
    async fn provider_selection_update_clears_exact_pin_without_route_switch() {
        let controller = RouteController::new(
            ProviderRoute::Serving {
                model: "anthropic:claude-sonnet-4-6".into(),
            },
            Box::new(NullBridge),
            None,
        );

        let snapshot = controller
            .set_provider_selection(ProviderSelection::Local)
            .await;

        assert_eq!(
            snapshot.serving_model(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(snapshot.intent.provider_selection, ProviderSelection::Local);
        assert_eq!(snapshot.intent.exact_model_override, None);
    }

    #[tokio::test]
    async fn grade_policy_update_clears_exact_pin_without_route_switch() {
        let controller = RouteController::new(
            ProviderRoute::Serving {
                model: "anthropic:claude-sonnet-4-6".into(),
            },
            Box::new(NullBridge),
            None,
        );

        let snapshot = controller.set_grade_policy(GradePolicy::Exact).await;

        assert_eq!(
            snapshot.serving_model(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(snapshot.intent.grade_policy, GradePolicy::Exact);
        assert_eq!(snapshot.intent.exact_model_override, None);
    }
    #[test]
    fn model_intent_grade_maps_to_capability_request() {
        let intent = ModelIntent::with_grade(ModelGrade::S);
        let req = intent.to_capability_request();
        assert_eq!(req.grade, crate::routing::CapabilityGradeBand::Max);
        assert!(req.only_providers.is_empty());
    }

    #[test]
    fn explicit_provider_policy_overrides_provider_selection_policy() {
        let mut intent = ModelIntent::with_grade(ModelGrade::B);
        intent.provider_selection = ProviderSelection::Local;
        intent.provider_policy = Some(crate::semantic_route::ProviderPolicy::CopilotOnly);

        assert_eq!(
            intent.to_provider_policy(),
            crate::semantic_route::ProviderPolicy::CopilotOnly
        );
    }

    #[test]
    fn model_intent_local_provider_maps_to_ollama_only() {
        let mut intent = ModelIntent::with_grade(ModelGrade::D);
        intent.provider_selection = ProviderSelection::Local;
        let req = intent.to_capability_request();
        assert_eq!(req.only_providers, vec!["ollama"]);
        assert!(req.prefer_local);
    }

    #[test]
    fn model_intent_endpoint_provider_maps_to_only_provider() {
        let mut intent = ModelIntent::with_grade(ModelGrade::B);
        intent.provider_selection = ProviderSelection::Endpoint("groq".into());
        let req = intent.to_capability_request();
        assert_eq!(req.only_providers, vec!["groq"]);
    }

    #[test]
    fn model_intent_exact_override_restricts_to_pinned_provider() {
        let intent = ModelIntent::pinned_model("openai-codex:gpt-5.4".into());
        let req = intent.to_capability_request();
        assert_eq!(req.only_providers, vec!["openai-codex"]);
        assert_eq!(req.grade, crate::routing::CapabilityGradeBand::Max);
    }
    fn intent_test_inventory() -> crate::routing::ProviderInventory {
        crate::routing::ProviderInventory {
            entries: vec![
                crate::routing::ProviderEntry {
                    provider_id: "anthropic".into(),
                    capability_grade: crate::routing::CapabilityGradeBand::Max,
                    has_credentials: true,
                    is_reachable: true,
                    models: vec!["claude-fable-5".into()],
                },
                crate::routing::ProviderEntry {
                    provider_id: "groq".into(),
                    capability_grade: crate::routing::CapabilityGradeBand::Mid,
                    has_credentials: true,
                    is_reachable: true,
                    models: vec!["llama-3.3-70b-versatile".into()],
                },
                crate::routing::ProviderEntry {
                    provider_id: "ollama".into(),
                    capability_grade: crate::routing::CapabilityGradeBand::Mid,
                    has_credentials: true,
                    is_reachable: true,
                    models: vec!["qwen3:30b".into()],
                },
            ],
            ollama_models: vec![crate::routing::OllamaModelInfo {
                name: "qwen3:30b".into(),
                size_bytes: 30_000_000_000,
                is_running: true,
                vram_bytes: 20_000_000_000,
            }],
            probed_at: std::time::Instant::now(),
        }
    }

    #[test]
    fn select_candidate_for_local_intent_uses_ollama() {
        let mut intent = ModelIntent::with_grade(ModelGrade::D);
        intent.provider_selection = ProviderSelection::Local;
        let candidate = select_candidate_for_intent(&intent, &intent_test_inventory()).unwrap();
        assert_eq!(candidate.provider_id, "ollama");
    }

    #[test]
    fn select_candidate_for_endpoint_intent_uses_endpoint() {
        let mut intent = ModelIntent::with_grade(ModelGrade::D);
        intent.provider_selection = ProviderSelection::Endpoint("groq".into());
        let candidate = select_candidate_for_intent(&intent, &intent_test_inventory()).unwrap();
        assert_eq!(candidate.provider_id, "groq");
    }

    #[test]
    fn select_candidate_for_upstream_intent_excludes_ollama() {
        let mut intent = ModelIntent::with_grade(ModelGrade::D);
        intent.provider_selection = ProviderSelection::Upstream;
        let candidate = select_candidate_for_intent(&intent, &intent_test_inventory()).unwrap();
        assert_ne!(candidate.provider_id, "ollama");
    }

    #[tokio::test]
    async fn resolve_route_from_intent_candidate_preserves_intent() {
        let controller = RouteController::with_initial_intent(
            ProviderRoute::Serving {
                model: "anthropic:claude-sonnet-4-6".into(),
            },
            Box::new(NullBridge),
            None,
            ModelIntent {
                grade: Some(ModelGrade::S),
                provider_selection: ProviderSelection::Local,
                ..ModelIntent::default()
            },
        );
        let candidate = crate::routing::ProviderCandidate {
            provider_id: "ollama".into(),
            model_id: "qwen3:30b".into(),
            score: 10.0,
        };

        let snapshot = controller
            .resolve_route_from_intent_candidate(candidate, Box::new(NullBridge))
            .await
            .unwrap();

        assert_eq!(snapshot.serving_model(), Some("ollama:qwen3:30b"));
        assert_eq!(snapshot.intent.grade, Some(ModelGrade::S));
        assert_eq!(snapshot.intent.provider_selection, ProviderSelection::Local);
    }

    #[tokio::test]
    async fn resolve_route_from_intent_inventory_preserves_route_without_candidate() {
        let controller = RouteController::with_initial_intent(
            ProviderRoute::Serving {
                model: "anthropic:claude-sonnet-4-6".into(),
            },
            Box::new(NullBridge),
            None,
            ModelIntent {
                grade: Some(ModelGrade::S),
                provider_selection: ProviderSelection::Endpoint("missing".into()),
                ..ModelIntent::default()
            },
        );

        let snapshot = controller
            .resolve_route_from_intent_inventory(&intent_test_inventory(), Box::new(NullBridge))
            .await
            .unwrap();

        assert_eq!(
            snapshot.serving_model(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(
            snapshot.intent.provider_selection,
            ProviderSelection::Endpoint("missing".into())
        );
        assert!(
            snapshot
                .warning
                .as_deref()
                .unwrap_or("")
                .contains("No provider candidate")
        );
    }
}
