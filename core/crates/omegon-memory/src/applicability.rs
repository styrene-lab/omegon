//! Applicability is independent of confidence, provenance, and presentation.
use crate::*;

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= 2048
        && !value.chars().any(char::is_control)
}
fn timestamp(value: &str) -> backend::Result<chrono::DateTime<chrono::FixedOffset>> {
    if value.len() > 64 {
        return Err(MemoryError::InvalidMutation(
            "applicability timestamp exceeds its bound".into(),
        ));
    }
    chrono::DateTime::parse_from_rfc3339(value).map_err(|_| {
        MemoryError::InvalidMutation("applicability timestamps must be RFC3339".into())
    })
}
fn revision(value: &str) -> bool {
    value.strip_prefix("git:").is_some_and(|oid| {
        matches!(oid.len(), 40 | 64)
            && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
            && oid == oid.to_ascii_lowercase()
    })
}
impl ApplicabilityConstraints {
    pub fn validate(&self) -> backend::Result<()> {
        for values in [
            &self.platforms,
            &self.workspaces,
            &self.revisions,
            &self.components,
        ] {
            if values.len() > 16 || values.iter().any(|value| !identifier(value)) {
                return Err(MemoryError::InvalidMutation(
                    "invalid applicability identifiers".into(),
                ));
            }
        }
        if [
            &self.platforms,
            &self.workspaces,
            &self.revisions,
            &self.components,
        ]
        .into_iter()
        .flatten()
        .map(String::len)
        .sum::<usize>()
            > 16_384
        {
            return Err(MemoryError::InvalidMutation(
                "applicability identifiers exceed aggregate bound".into(),
            ));
        }
        if self.platforms.iter().any(|value| {
            value.len() > 32
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        }) || self.revisions.iter().any(|value| !revision(value))
        {
            return Err(MemoryError::InvalidMutation("platforms use lowercase OS identifiers; revisions use git:<full lowercase commit OID>".into()));
        }
        let start = self.valid_from.as_deref().map(timestamp).transpose()?;
        let end = self.valid_until.as_deref().map(timestamp).transpose()?;
        if start.zip(end).is_some_and(|(start, end)| start >= end) {
            return Err(MemoryError::InvalidMutation(
                "applicability validity interval must be nonempty".into(),
            ));
        }
        Ok(())
    }

    pub fn assess(&self, context: &ApplicabilityContext) -> ApplicabilityStatus {
        if self.validate().is_err() || context.validate().is_err() {
            return ApplicabilityStatus::Inapplicable;
        }
        let mut constrained = false;
        let mut unknown = false;
        for (allowed, actual) in [
            (&self.platforms, &context.platform),
            (&self.workspaces, &context.workspace),
            (&self.revisions, &context.revision),
            (&self.components, &context.component),
        ] {
            if allowed.is_empty() {
                continue;
            }
            constrained = true;
            match actual {
                Some(actual) if !allowed.contains(actual) => {
                    return ApplicabilityStatus::Inapplicable;
                }
                None => unknown = true,
                _ => {}
            }
        }
        if self.valid_from.is_some() || self.valid_until.is_some() {
            constrained = true;
            if let Some(at) = context
                .at
                .as_deref()
                .and_then(|value| timestamp(value).ok())
            {
                if self
                    .valid_from
                    .as_deref()
                    .and_then(|value| timestamp(value).ok())
                    .is_some_and(|start| at < start)
                    || self
                        .valid_until
                        .as_deref()
                        .and_then(|value| timestamp(value).ok())
                        .is_some_and(|end| at >= end)
                {
                    return ApplicabilityStatus::Inapplicable;
                }
            } else {
                unknown = true;
            }
        }
        if !constrained || unknown {
            ApplicabilityStatus::Unknown
        } else {
            ApplicabilityStatus::Matches
        }
    }
}

pub fn context_arg(args: &serde_json::Value) -> backend::Result<Option<ApplicabilityContext>> {
    let context = args
        .get("context")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<ApplicabilityContext>(value.clone()))
        .transpose()
        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
    if let Some(context) = &context {
        context.validate()?;
    }
    Ok(context)
}
pub fn constraints_arg(
    args: &serde_json::Value,
) -> backend::Result<Option<ApplicabilityConstraints>> {
    let constraints = args
        .get("applicability")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<ApplicabilityConstraints>(value.clone()))
        .transpose()
        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
    if let Some(constraints) = &constraints {
        constraints.validate()?;
    }
    Ok(constraints)
}
pub fn constraints_schema() -> serde_json::Value {
    serde_json::json!({"type":"object","additionalProperties":false,"properties":{
        "platforms":{"type":"array","items":{"type":"string"},"maxItems":16},
        "workspaces":{"type":"array","items":{"type":"string"},"maxItems":16},
        "revisions":{"type":"array","items":{"type":"string"},"maxItems":16,"description":"Exact git:<full lowercase commit OID> identities"},
        "components":{"type":"array","items":{"type":"string"},"maxItems":16},
        "valid_from":{"type":"string","description":"Inclusive RFC3339 valid time"},
        "valid_until":{"type":"string","description":"Exclusive RFC3339 valid time"}}})
}
pub fn context_schema() -> serde_json::Value {
    serde_json::json!({"type":"object","additionalProperties":false,"properties":{
        "platform":{"type":"string"},"workspace":{"type":"string"},"revision":{"type":"string"},"component":{"type":"string"},
        "at":{"type":"string","description":"RFC3339 valid-time query; omitted historical time is unknown"}}})
}
impl RecordedApplicability {
    pub fn validate(&self) -> backend::Result<()> {
        self.constraints.validate()?;
        timestamp(&self.recorded_at)?;
        Ok(())
    }
    pub fn new(constraints: ApplicabilityConstraints) -> backend::Result<Self> {
        constraints.validate()?;
        Ok(Self {
            constraints,
            recorded_at: util::now_iso(),
        })
    }
}
impl ApplicabilityContext {
    pub fn local() -> Self {
        Self {
            platform: Some(std::env::consts::OS.into()),
            at: Some(util::now_iso()),
            ..Default::default()
        }
    }
    pub fn validate(&self) -> backend::Result<()> {
        for value in [
            &self.platform,
            &self.workspace,
            &self.revision,
            &self.component,
        ]
        .into_iter()
        .flatten()
        {
            if !identifier(value) {
                return Err(MemoryError::InvalidMutation(
                    "invalid applicability context".into(),
                ));
            }
        }
        if self.platform.as_ref().is_some_and(|value| {
            value.len() > 32
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        }) {
            return Err(MemoryError::InvalidMutation(
                "platform context must use a lowercase OS identifier".into(),
            ));
        }
        if self
            .revision
            .as_deref()
            .is_some_and(|value| !revision(value))
        {
            return Err(MemoryError::InvalidMutation(
                "invalid applicability revision context".into(),
            ));
        }
        if let Some(at) = &self.at {
            timestamp(at)?;
        }
        Ok(())
    }
}
impl SearchFilter {
    pub fn resolved(&self) -> backend::Result<Self> {
        let mut context = self.context.clone().unwrap_or_else(|| {
            if self.intent == SearchIntent::Current {
                ApplicabilityContext::local()
            } else {
                ApplicabilityContext::default()
            }
        });
        if self.intent == SearchIntent::Current && context.at.is_none() {
            context.at = Some(util::now_iso());
        }
        context.validate()?;
        Ok(Self {
            context: Some(context),
            ..self.clone()
        })
    }
    pub fn applicability_status(&self, fact: &Fact) -> ApplicabilityStatus {
        let Some(applicability) = &fact.applicability else {
            return ApplicabilityStatus::Unknown;
        };
        let context = self.context.clone().unwrap_or_else(|| {
            if self.intent == SearchIntent::Current {
                ApplicabilityContext::local()
            } else {
                ApplicabilityContext::default()
            }
        });
        applicability.constraints.assess(&context)
    }
}
