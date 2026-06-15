//! Runtime authority presets for prompt and operation policy.

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
