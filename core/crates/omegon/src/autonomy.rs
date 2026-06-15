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
    fn autonomy_levels_have_stable_wire_labels() {
        assert_eq!(AutonomyLevel::Manual.as_str(), "manual");
        assert_eq!(AutonomyLevel::Conservative.as_str(), "conservative");
        assert_eq!(AutonomyLevel::Autonomous.as_str(), "autonomous");
        assert_eq!(AutonomyLevel::Orchestrator.as_str(), "orchestrator");
        assert_eq!(AutonomyLevel::Batch.as_str(), "batch");
    }
}
