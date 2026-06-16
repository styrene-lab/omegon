//! Runtime authority presets for prompt and operation policy.

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyLevel {
    Manual,
    Conservative,
    Autonomous,
    Orchestrator,
    Batch,
}

impl AutonomyLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Conservative => "conservative",
            Self::Autonomous => "autonomous",
            Self::Orchestrator => "orchestrator",
            Self::Batch => "batch",
        }
    }
}

impl Default for AutonomyLevel {
    fn default() -> Self {
        Self::Conservative
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionPolicy {
    Deny,
    RequireApproval,
    Allow,
}

impl DecisionPolicy {
    pub fn prompt_label(self) -> &'static str {
        match self {
            Self::Deny => "denied unless explicitly requested",
            Self::RequireApproval => "requires structured approval",
            Self::Allow => "allowed when justified",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomySource {
    Session,
    Loop,
    ScheduledJob,
    ExplicitApproval,
}

impl AutonomySource {
    pub fn precedence(self) -> u8 {
        match self {
            Self::Session => 10,
            Self::Loop => 20,
            Self::ScheduledJob => 20,
            Self::ExplicitApproval => 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityOperation {
    DelegateScout,
    DelegatePatch,
    DelegateVerify,
    CleaveAssess,
    CleaveRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutonomyEnvelope {
    pub source: AutonomySource,
    pub level: AutonomyLevel,
    pub allowed_operations: Vec<AuthorityOperation>,
    pub denied_operations: Vec<AuthorityOperation>,
    pub max_turns: Option<usize>,
    pub max_wall_time_secs: Option<u64>,
    pub max_delegate_tasks: Option<usize>,
    pub max_cleave_children: Option<usize>,
    pub max_parallel: Option<usize>,
    pub execution_substrate: Option<String>,
}

impl AutonomyEnvelope {
    pub fn session(level: AutonomyLevel) -> Self {
        Self {
            source: AutonomySource::Session,
            level,
            allowed_operations: Vec::new(),
            denied_operations: Vec::new(),
            max_turns: None,
            max_wall_time_secs: None,
            max_delegate_tasks: None,
            max_cleave_children: None,
            max_parallel: None,
            execution_substrate: None,
        }
    }

    pub fn loop_run(level: AutonomyLevel) -> Self {
        Self {
            source: AutonomySource::Loop,
            ..Self::session(level)
        }
    }

    pub fn scheduled_job(level: AutonomyLevel) -> Self {
        Self {
            source: AutonomySource::ScheduledJob,
            ..Self::session(level)
        }
    }

    pub fn explicit_approval(level: AutonomyLevel) -> Self {
        Self {
            source: AutonomySource::ExplicitApproval,
            ..Self::session(level)
        }
    }
}

pub fn resolve_autonomy_envelope<'a>(
    envelopes: impl IntoIterator<Item = &'a AutonomyEnvelope>,
) -> AutonomyEnvelope {
    envelopes
        .into_iter()
        .max_by_key(|envelope| envelope.source.precedence())
        .cloned()
        .unwrap_or_else(|| AutonomyEnvelope::session(AutonomyLevel::Conservative))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentPolicy {
    pub level: AutonomyLevel,
    pub delegate_scout: DecisionPolicy,
    pub delegate_patch: DecisionPolicy,
    pub delegate_verify: DecisionPolicy,
    pub cleave_assess: DecisionPolicy,
    pub cleave_run: DecisionPolicy,
    pub max_children: usize,
    pub max_parallel: usize,
}

impl SubagentPolicy {
    pub fn for_level(level: AutonomyLevel) -> Self {
        match level {
            AutonomyLevel::Manual => Self {
                level,
                delegate_scout: DecisionPolicy::RequireApproval,
                delegate_patch: DecisionPolicy::RequireApproval,
                delegate_verify: DecisionPolicy::RequireApproval,
                cleave_assess: DecisionPolicy::RequireApproval,
                cleave_run: DecisionPolicy::RequireApproval,
                max_children: 1,
                max_parallel: 1,
            },
            AutonomyLevel::Conservative => Self {
                level,
                delegate_scout: DecisionPolicy::Allow,
                delegate_patch: DecisionPolicy::RequireApproval,
                delegate_verify: DecisionPolicy::Allow,
                cleave_assess: DecisionPolicy::Allow,
                cleave_run: DecisionPolicy::RequireApproval,
                max_children: 2,
                max_parallel: 1,
            },
            AutonomyLevel::Autonomous => Self {
                level,
                delegate_scout: DecisionPolicy::Allow,
                delegate_patch: DecisionPolicy::Allow,
                delegate_verify: DecisionPolicy::Allow,
                cleave_assess: DecisionPolicy::Allow,
                cleave_run: DecisionPolicy::Allow,
                max_children: 4,
                max_parallel: 2,
            },
            AutonomyLevel::Orchestrator => Self {
                level,
                delegate_scout: DecisionPolicy::Allow,
                delegate_patch: DecisionPolicy::Allow,
                delegate_verify: DecisionPolicy::Allow,
                cleave_assess: DecisionPolicy::Allow,
                cleave_run: DecisionPolicy::Allow,
                max_children: 8,
                max_parallel: 4,
            },
            AutonomyLevel::Batch => Self {
                level,
                delegate_scout: DecisionPolicy::Allow,
                delegate_patch: DecisionPolicy::Allow,
                delegate_verify: DecisionPolicy::Allow,
                cleave_assess: DecisionPolicy::Allow,
                cleave_run: DecisionPolicy::Allow,
                max_children: 8,
                max_parallel: 4,
            },
        }
    }

    pub fn conservative_default() -> Self {
        Self::for_level(AutonomyLevel::Conservative)
    }
}

pub fn subagent_level_for_automation(level: crate::settings::AutomationLevel) -> AutonomyLevel {
    match level {
        crate::settings::AutomationLevel::Ask => AutonomyLevel::Manual,
        crate::settings::AutomationLevel::Guarded => AutonomyLevel::Conservative,
        crate::settings::AutomationLevel::Flow => AutonomyLevel::Conservative,
        crate::settings::AutomationLevel::Autonomous => AutonomyLevel::Orchestrator,
    }
}

pub fn subagent_policy_for_automation(level: crate::settings::AutomationLevel) -> SubagentPolicy {
    SubagentPolicy::for_level(subagent_level_for_automation(level))
}

/// Resolve the active subagent authority policy for the current runtime.
///
/// This is intentionally centralized even while it returns the conservative
/// default so prompt generation, delegate gates, and cleave gates cannot drift.
/// Future CLI/config/profile autonomy settings should be wired here first.
pub fn active_subagent_policy() -> SubagentPolicy {
    SubagentPolicy::conservative_default()
}

pub struct ApprovalRequest<'a> {
    pub operation: &'a str,
    pub reason: &'a str,
    pub requested: Value,
    pub allowed: Value,
    pub grants: Vec<omegon_traits::AuthorityGrant>,
}

/// Build compatibility approval details: legacy flat fields plus the SDK-backed
/// `required_approval` object that future TUI/ACP/Web approval paths can consume.
pub fn required_approval_details(policy: &SubagentPolicy, request: ApprovalRequest<'_>) -> Value {
    let approval = omegon_traits::RequiredApproval {
        kind: omegon_traits::RequiredApprovalKind::ApprovalRequired,
        operation: request.operation.to_string(),
        reason: request.reason.to_string(),
        autonomy: to_sdk_autonomy(policy.level),
        requested: request.requested.clone(),
        allowed: request.allowed.clone(),
        choices: vec![omegon_traits::ApprovalChoice {
            id: "approve_once".into(),
            label: "Approve once".into(),
            scope: omegon_traits::ApprovalScope::Once,
            grants: request.grants,
        }],
    };

    json!({
        "approval_required": true,
        "operation": request.operation,
        "autonomy": policy.level.as_str(),
        "reason": request.reason,
        "requested": request.requested,
        "allowed": request.allowed,
        "required_approval": approval,
    })
}

fn to_sdk_autonomy(level: AutonomyLevel) -> omegon_traits::AutonomyLevel {
    match level {
        AutonomyLevel::Manual => omegon_traits::AutonomyLevel::Manual,
        AutonomyLevel::Conservative => omegon_traits::AutonomyLevel::Conservative,
        AutonomyLevel::Autonomous => omegon_traits::AutonomyLevel::Autonomous,
        AutonomyLevel::Orchestrator => omegon_traits::AutonomyLevel::Orchestrator,
        AutonomyLevel::Batch => omegon_traits::AutonomyLevel::Batch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_policy_defaults_to_conservative() {
        let policy = active_subagent_policy();
        assert_eq!(policy.level, AutonomyLevel::Conservative);
        assert_eq!(policy.delegate_scout, DecisionPolicy::Allow);
        assert_eq!(policy.delegate_patch, DecisionPolicy::RequireApproval);
        assert_eq!(policy.delegate_verify, DecisionPolicy::Allow);
        assert_eq!(policy.cleave_assess, DecisionPolicy::Allow);
        assert_eq!(policy.cleave_run, DecisionPolicy::RequireApproval);
        assert_eq!(policy.max_children, 2);
        assert_eq!(policy.max_parallel, 1);
    }

    #[test]
    fn autonomy_envelope_resolution_defaults_to_conservative_session() {
        let resolved = resolve_autonomy_envelope(std::iter::empty());
        assert_eq!(resolved.source, AutonomySource::Session);
        assert_eq!(resolved.level, AutonomyLevel::Conservative);
    }

    #[test]
    fn loop_envelope_overrides_session_without_increasing_by_trigger_alone() {
        let session = AutonomyEnvelope::session(AutonomyLevel::Orchestrator);
        let mut loop_envelope = AutonomyEnvelope::loop_run(AutonomyLevel::Conservative);
        loop_envelope.max_turns = Some(5);
        loop_envelope.denied_operations = vec![AuthorityOperation::CleaveRun];

        let resolved = resolve_autonomy_envelope([&session, &loop_envelope]);
        assert_eq!(resolved.source, AutonomySource::Loop);
        assert_eq!(resolved.level, AutonomyLevel::Conservative);
        assert_eq!(resolved.max_turns, Some(5));
        assert_eq!(resolved.denied_operations, vec![AuthorityOperation::CleaveRun]);
    }

    #[test]
    fn scheduled_job_envelope_overrides_session_policy() {
        let session = AutonomyEnvelope::session(AutonomyLevel::Orchestrator);
        let mut job = AutonomyEnvelope::scheduled_job(AutonomyLevel::Manual);
        job.allowed_operations = vec![AuthorityOperation::DelegateVerify];
        job.denied_operations = vec![AuthorityOperation::DelegatePatch, AuthorityOperation::CleaveRun];

        let resolved = resolve_autonomy_envelope([&session, &job]);
        assert_eq!(resolved.source, AutonomySource::ScheduledJob);
        assert_eq!(resolved.level, AutonomyLevel::Manual);
        assert_eq!(resolved.allowed_operations, vec![AuthorityOperation::DelegateVerify]);
        assert_eq!(
            resolved.denied_operations,
            vec![AuthorityOperation::DelegatePatch, AuthorityOperation::CleaveRun]
        );
    }

    #[test]
    fn explicit_approval_has_highest_precedence() {
        let session = AutonomyEnvelope::session(AutonomyLevel::Manual);
        let loop_envelope = AutonomyEnvelope::loop_run(AutonomyLevel::Conservative);
        let approval = AutonomyEnvelope::explicit_approval(AutonomyLevel::Orchestrator);

        let resolved = resolve_autonomy_envelope([&session, &loop_envelope, &approval]);
        assert_eq!(resolved.source, AutonomySource::ExplicitApproval);
        assert_eq!(resolved.level, AutonomyLevel::Orchestrator);
    }

    #[test]
    fn automation_levels_map_to_subagent_autonomy_levels() {
        assert_eq!(
            subagent_level_for_automation(crate::settings::AutomationLevel::Ask),
            AutonomyLevel::Manual
        );
        assert_eq!(
            subagent_level_for_automation(crate::settings::AutomationLevel::Guarded),
            AutonomyLevel::Conservative
        );
        assert_eq!(
            subagent_level_for_automation(crate::settings::AutomationLevel::Flow),
            AutonomyLevel::Conservative
        );
        assert_eq!(
            subagent_level_for_automation(crate::settings::AutomationLevel::Autonomous),
            AutonomyLevel::Orchestrator
        );
    }

    #[test]
    fn automation_policy_mapping_preserves_conservative_default() {
        let policy = subagent_policy_for_automation(crate::settings::AutomationLevel::Guarded);
        assert_eq!(policy.level, AutonomyLevel::Conservative);
        assert_eq!(policy.delegate_scout, DecisionPolicy::Allow);
        assert_eq!(policy.delegate_patch, DecisionPolicy::RequireApproval);
        assert_eq!(policy.cleave_run, DecisionPolicy::RequireApproval);
        assert_eq!(policy.max_children, 2);
        assert_eq!(policy.max_parallel, 1);
    }

    #[test]
    fn autonomy_levels_have_stable_wire_labels() {
        assert_eq!(AutonomyLevel::Manual.as_str(), "manual");
        assert_eq!(AutonomyLevel::Conservative.as_str(), "conservative");
        assert_eq!(AutonomyLevel::Autonomous.as_str(), "autonomous");
        assert_eq!(AutonomyLevel::Orchestrator.as_str(), "orchestrator");
        assert_eq!(AutonomyLevel::Batch.as_str(), "batch");
    }
}
