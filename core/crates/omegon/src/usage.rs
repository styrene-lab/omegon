//! Shared provider-usage summarization.
//!
//! Builds conservative operator-facing summaries from the already captured
//! `ProviderTelemetrySnapshot`. No upstream polling is performed here.

use omegon_traits::ProviderTelemetrySnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageHeadroomState {
    Unknown,
    Healthy,
    Elevated,
    Constrained,
    Exhausted,
}

impl UsageHeadroomState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Healthy => "healthy",
            Self::Elevated => "elevated",
            Self::Constrained => "constrained",
            Self::Exhausted => "exhausted",
        }
    }

    pub fn compact_label(&self) -> &'static str {
        match self {
            Self::Unknown => "?",
            Self::Healthy => "ok",
            Self::Elevated => "elev",
            Self::Constrained => "tight",
            Self::Exhausted => "full",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageAuthorityLink {
    pub label: &'static str,
    pub url: &'static str,
}

pub fn classify_pct(pct: f32) -> UsageHeadroomState {
    if pct >= 98.0 {
        UsageHeadroomState::Exhausted
    } else if pct >= 90.0 {
        UsageHeadroomState::Constrained
    } else if pct >= 70.0 {
        UsageHeadroomState::Elevated
    } else {
        UsageHeadroomState::Healthy
    }
}

pub fn derive_headroom_state(telemetry: Option<&ProviderTelemetrySnapshot>) -> UsageHeadroomState {
    let Some(t) = telemetry else {
        return UsageHeadroomState::Unknown;
    };

    match t.provider.as_str() {
        "anthropic" => t
            .unified_5h_utilization_pct
            .map(classify_pct)
            .or_else(|| t.unified_7d_utilization_pct.map(classify_pct))
            .unwrap_or(UsageHeadroomState::Unknown),
        "openai-codex" => t
            .codex_primary_used_pct
            .map(classify_pct)
            .unwrap_or(UsageHeadroomState::Unknown),
        _ => {
            if let Some(secs) = t.retry_after_secs
                && secs > 0
                && (t.requests_remaining == Some(0) || t.tokens_remaining == Some(0))
            {
                return UsageHeadroomState::Exhausted;
            }
            if let Some(req) = t.requests_remaining
                && req == 0
            {
                return UsageHeadroomState::Exhausted;
            }
            if let Some(tok) = t.tokens_remaining
                && tok == 0
            {
                return UsageHeadroomState::Exhausted;
            }
            if t.retry_after_secs.is_some() {
                return UsageHeadroomState::Constrained;
            }
            if t.requests_remaining.is_some() || t.tokens_remaining.is_some() {
                return UsageHeadroomState::Elevated;
            }
            UsageHeadroomState::Unknown
        }
    }
}

pub fn derive_rationale(
    telemetry: Option<&ProviderTelemetrySnapshot>,
    headroom: &UsageHeadroomState,
) -> String {
    let Some(t) = telemetry else {
        return "no provider telemetry has been captured yet in this session".to_string();
    };

    match t.provider.as_str() {
        "anthropic" => match (t.unified_5h_utilization_pct, t.unified_7d_utilization_pct) {
            (Some(short), Some(long)) => format!(
                "derived from Anthropic upstream utilization headers: 5h {:.0}% and 7d {:.0}%",
                short, long
            ),
            (Some(short), None) => format!(
                "derived from Anthropic upstream 5h utilization header: {:.0}%",
                short
            ),
            (None, Some(long)) => format!(
                "derived from Anthropic upstream 7d utilization header: {:.0}%",
                long
            ),
            _ => {
                "Anthropic provider selected, but no utilization headers were captured".to_string()
            }
        },
        "openai-codex" => match t.codex_primary_used_pct {
            Some(pct) => format!(
                "derived from Codex primary window used-percent header: {:.0}% used ({:.0}% remaining){}",
                pct,
                (100.0 - pct).clamp(0.0, 100.0),
                t.codex_primary_reset_secs
                    .map(|secs| format!(", reset in {}", format_duration_compact(secs)))
                    .unwrap_or_default()
            ),
            None => "Codex provider selected, but no primary used-percent header was captured"
                .to_string(),
        },
        _ => {
            let mut parts = Vec::new();
            if let Some(req) = t.requests_remaining {
                parts.push(format!("requests remaining {req}"));
            }
            if let Some(tok) = t.tokens_remaining {
                parts.push(format!("tokens remaining {}", format_compact_tokens(tok)));
            }
            if let Some(secs) = t.retry_after_secs {
                parts.push(format!("retry-after {}", format_duration_compact(secs)));
            }
            if parts.is_empty() {
                format!(
                    "no recognized quota headers captured; advisory remains {}",
                    headroom.as_str()
                )
            } else {
                format!(
                    "best-effort advisory from generic quota headers: {}",
                    parts.join(", ")
                )
            }
        }
    }
}

pub fn format_raw_telemetry_lines(t: &ProviderTelemetrySnapshot) -> Vec<String> {
    let mut lines = Vec::new();
    match t.provider.as_str() {
        "anthropic" => {
            if let Some(pct) = t.unified_5h_utilization_pct {
                lines.push(format!("5h utilization: {:.0}%", pct));
            }
            if let Some(pct) = t.unified_7d_utilization_pct {
                lines.push(format!("7d utilization: {:.0}%", pct));
            }
            if let Some(secs) = t.retry_after_secs {
                lines.push(format!("retry after: {}", format_duration_compact(secs)));
            }
        }
        "openai-codex" => {
            if let Some(name) = &t.codex_limit_name {
                lines.push(format!("model limit: {name}"));
            }
            if let Some(active) = &t.codex_active_limit {
                lines.push(format!("active limit: {active}"));
            }
            if let Some(pct) = t.codex_primary_used_pct {
                lines.push(format!("primary used: {:.0}%", pct));
                lines.push(format!(
                    "primary remaining: {:.0}%",
                    (100.0 - pct).clamp(0.0, 100.0)
                ));
            }
            if let Some(pct) = t.codex_secondary_used_pct {
                lines.push(format!("secondary used: {:.0}%", pct));
                lines.push(format!(
                    "secondary remaining: {:.0}%",
                    (100.0 - pct).clamp(0.0, 100.0)
                ));
            }
            if let Some(secs) = t.codex_primary_reset_secs {
                lines.push(format!("primary reset: {}", format_duration_compact(secs)));
            }
            if let Some(secs) = t.codex_secondary_reset_secs {
                lines.push(format!(
                    "secondary reset: {}",
                    format_duration_compact(secs)
                ));
            }
            if let Some(unlimited) = t.codex_credits_unlimited {
                lines.push(format!(
                    "credits: {}",
                    if unlimited { "unlimited" } else { "metered" }
                ));
            }
        }
        _ => {
            if let Some(req) = t.requests_remaining {
                lines.push(format!("requests remaining: {req}"));
            }
            if let Some(tok) = t.tokens_remaining {
                lines.push(format!("tokens remaining: {}", format_compact_tokens(tok)));
            }
            if let Some(secs) = t.retry_after_secs {
                lines.push(format!("retry after: {}", format_duration_compact(secs)));
            }
        }
    }
    lines
}

pub fn authoritative_links(provider: &str) -> Vec<UsageAuthorityLink> {
    match provider {
        "anthropic" => vec![UsageAuthorityLink {
            label: "Anthropic rate limits",
            url: "https://platform.claude.com/docs/en/api/rate-limits",
        }],
        "openai" => vec![UsageAuthorityLink {
            label: "OpenAI API rate limits",
            url: "https://developers.openai.com/api/docs/guides/rate-limits",
        }],
        "openai-codex" => vec![
            UsageAuthorityLink {
                label: "OpenAI help",
                url: "https://help.openai.com/en/?q=rate+limit",
            },
            UsageAuthorityLink {
                label: "OpenAI API rate limits",
                url: "https://developers.openai.com/api/docs/guides/rate-limits",
            },
        ],
        _ => Vec::new(),
    }
}

pub fn format_provider_telemetry_compact(
    telemetry: Option<&ProviderTelemetrySnapshot>,
) -> Option<String> {
    let t = telemetry?;
    let mut parts = Vec::new();
    match t.provider.as_str() {
        "anthropic" => {
            if let Some(pct) = t.unified_5h_utilization_pct {
                parts.push(format!("5h {:.0}%", pct));
            }
            if let Some(pct) = t.unified_7d_utilization_pct {
                parts.push(format!("7d {:.0}%", pct));
            }
            if let Some(secs) = t.retry_after_secs {
                parts.push(format!("retry {}", format_duration_compact(secs)));
            }
        }
        "openai-codex" => {
            let family = t.codex_active_limit.as_deref().unwrap_or("codex");
            if let Some(used) = t.codex_primary_used_pct {
                parts.push(format!(
                    "{family} {:.0}% left",
                    (100.0 - used).clamp(0.0, 100.0)
                ));
            } else {
                parts.push(family.to_string());
            }
            if let Some(used) = t.codex_secondary_used_pct {
                parts.push(format!("7d {:.0}% left", (100.0 - used).clamp(0.0, 100.0)));
            } else if let Some(secs) = t.codex_secondary_reset_secs {
                parts.push(format!("weekly {}", format_duration_compact(secs)));
            }
            if let Some(secs) = t.codex_primary_reset_secs
                && matches!(
                    derive_headroom_state(Some(t)),
                    UsageHeadroomState::Constrained | UsageHeadroomState::Exhausted
                )
            {
                parts.push(format!("resets {}", format_duration_compact(secs)));
            }
            if let Some(unlimited) = t.codex_credits_unlimited {
                parts.push(if unlimited {
                    "credits ∞".into()
                } else {
                    "credits metered".into()
                });
            }
        }
        _ => {
            if let Some(rem) = t.requests_remaining {
                parts.push(format!("req {rem}"));
            }
            if let Some(rem) = t.tokens_remaining {
                parts.push(format!("tok {}", format_compact_tokens(rem)));
            }
            if let Some(secs) = t.retry_after_secs {
                parts.push(format!("retry {}", format_duration_compact(secs)));
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        let state = derive_headroom_state(Some(t));
        parts.push(state.compact_label().to_string());
        Some(parts.join(" · "))
    }
}

pub fn format_duration_compact(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    if hours < 24 {
        if rem_mins > 0 {
            return format!("{hours}h{rem_mins:02}m");
        }
        return format!("{hours}h");
    }
    let days = hours / 24;
    let rem_hours = hours % 24;
    if rem_hours > 0 {
        format!("{days}d{rem_hours}h")
    } else {
        format!("{days}d")
    }
}

pub fn format_compact_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.0}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_formatter_includes_state_suffix_for_anthropic() {
        let text = format_provider_telemetry_compact(Some(&ProviderTelemetrySnapshot {
            provider: "anthropic".into(),
            source: "response_headers".into(),
            unified_5h_utilization_pct: Some(42.0),
            unified_7d_utilization_pct: Some(64.0),
            ..Default::default()
        }))
        .expect("compact line");
        assert!(text.contains("5h 42%"), "got {text}");
        assert!(text.contains("7d 64%"), "got {text}");
        assert!(text.ends_with("ok"), "got {text}");
    }

    #[test]
    fn compact_formatter_includes_state_suffix_for_codex() {
        let text = format_provider_telemetry_compact(Some(&ProviderTelemetrySnapshot {
            provider: "openai-codex".into(),
            source: "response_headers".into(),
            codex_active_limit: Some("codex".into()),
            codex_primary_used_pct: Some(99.0),
            codex_primary_reset_secs: Some(13648),
            ..Default::default()
        }))
        .expect("compact line");
        assert!(text.contains("codex 1% left"), "got {text}");
        assert!(text.contains("resets 3h47m"), "got {text}");
        assert!(text.ends_with("full"), "got {text}");
    }
}
