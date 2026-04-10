//! Upstream error taxonomy, retry classification, and persistence.

use serde::Serialize;
use std::path::PathBuf;

/// Explicit internal representation of upstream failures.
///
/// This is the harness-facing contract: classify unstable upstream prose into
/// stable Omegon semantics before deciding whether to retry, compact, repair
/// conversation state, request re-auth, or stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamErrorClass {
    RateLimited,
    ProviderOverloaded,
    Upstream5xx,
    Timeout,
    StalledStream,
    NetworkConnect,
    NetworkReset,
    Dns,
    DecodeBody,
    BridgeDropped,
    ContextOverflow,
    MalformedHistory,
    SessionExpired,
    AuthInvalid,
    QuotaExceeded,
    BadRequest,
    /// Model output entered a degenerate repetition loop.
    DegenerateOutput,
    /// Responses API returned response.incomplete (output truncated by
    /// max_output_tokens or content filter).
    ResponseIncomplete,
    /// Responses API returned response.cancelled (server-side cancellation).
    ResponseCancelled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryAction {
    RetrySameProvider,
    FailoverPreferred,
    CompactContext,
    RepairConversation,
    Reauthenticate,
    Fatal,
}

/// Retryable transient subset used by the bounded backoff loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransientFailureKind {
    RateLimited,
    ProviderOverloaded,
    Upstream5xx,
    Timeout,
    StalledStream,
    NetworkConnect,
    NetworkReset,
    Dns,
    DecodeBody,
    BridgeDropped,
    ResponseIncomplete,
    ResponseCancelled,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct UpstreamFailureLogEntry {
    pub(crate) timestamp: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) failure_kind: String,
    pub(crate) internal_class: String,
    pub(crate) recovery_action: RecoveryAction,
    pub(crate) attempt: u32,
    pub(crate) delay_ms: u64,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Copy)]
struct ErrorRule {
    providers: &'static [&'static str],
    class: UpstreamErrorClass,
    substrings: &'static [&'static str],
    word_tokens: &'static [&'static str],
}

const PROVIDER_ERROR_RULES: &[ErrorRule] = &[
    ErrorRule {
        providers: &["anthropic"],
        class: UpstreamErrorClass::ContextOverflow,
        substrings: &["extra usage is required for long context requests"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &["anthropic"],
        class: UpstreamErrorClass::ProviderOverloaded,
        substrings: &["overloaded_error"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &["openai-codex"],
        class: UpstreamErrorClass::SessionExpired,
        substrings: &[
            "session expired",
            "session has expired",
            "out of session",
            "please log in again",
            "login required",
            "token expired",
            "expired token",
        ],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &["openai-codex"],
        class: UpstreamErrorClass::AuthInvalid,
        substrings: &[
            "api.responses.write",
            "insufficient permissions",
            "missing scopes",
            "missing scope",
            "incorrect role in your organization",
            "correct role in your organization",
            "restricted api key",
        ],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[
            "openai",
            "openrouter",
            "groq",
            "xai",
            "mistral",
            "cerebras",
            "huggingface",
            "ollama",
        ],
        class: UpstreamErrorClass::BadRequest,
        substrings: &[
            "invalid_request_error",
            "unsupported_parameter",
            "bad request",
        ],
        word_tokens: &["400"],
    },
];

const GLOBAL_ERROR_RULES: &[ErrorRule] = &[
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::SessionExpired,
        substrings: &[
            "session expired",
            "session has expired",
            "out of session",
            "out-of-session",
            "please log in again",
        ],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::AuthInvalid,
        substrings: &[
            "invalid api key",
            "unauthorized",
            "forbidden",
            "authentication",
        ],
        word_tokens: &["401", "403"],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::QuotaExceeded,
        substrings: &["quota", "insufficient credits", "billing", "usage limit"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::RateLimited,
        substrings: &[
            "rate limit",
            "rate_limit",
            "too many requests",
            "retry-after",
            "requests per min",
            "tokens per min",
            "request limit reached",
            "try again in",
        ],
        word_tokens: &["429", "rpm", "tpm"],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::ProviderOverloaded,
        substrings: &[
            "overloaded",
            "capacity",
            "at capacity",
            "server is busy",
            "high demand",
            "currently unavailable due to load",
        ],
        word_tokens: &["529"],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::Upstream5xx,
        substrings: &[
            "error code: 520",
            "origin error",
            "origin unreachable",
            "gateway timeout",
            "upstream connect error",
            "disconnect/reset before headers",
            "server closed the connection without sending any data",
            "cf_bad_gateway",
        ],
        word_tokens: &["500", "502", "503", "504", "520", "521", "522", "523", "524", "525", "526", "530"],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::StalledStream,
        substrings: &["stream idle for", "connection may be stalled"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::Timeout,
        substrings: &["timeout", "timed out"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::NetworkConnect,
        substrings: &["connection refused", "connection closed"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::NetworkReset,
        substrings: &[
            "connection reset",
            "reset by peer",
            "broken pipe",
            "unexpected eof",
        ],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::Dns,
        substrings: &["dns error", "name resolution"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::DecodeBody,
        substrings: &[
            "error decoding response body",
            "decode response body",
            "failed to decode response body",
        ],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::BridgeDropped,
        substrings: &["stream ended without", "bridge may have crashed"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::Upstream5xx,
        substrings: &[
            "server_error",
            "temporarily",
            "try again",
            "service unavailable",
            "bad gateway",
            "internal server error",
        ],
        word_tokens: &["500", "502", "503"],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::BadRequest,
        substrings: &["invalid request", "bad request"],
        word_tokens: &["400"],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::DegenerateOutput,
        substrings: &["degenerate", "repeated"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::ResponseIncomplete,
        substrings: &["response incomplete", "output was truncated"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::ResponseCancelled,
        substrings: &["response cancelled", "cancelled by server"],
        word_tokens: &[],
    },
    ErrorRule {
        providers: &[],
        class: UpstreamErrorClass::BridgeDropped,
        substrings: &["stream closed without completion"],
        word_tokens: &[],
    },
];

impl UpstreamErrorClass {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::RateLimited => "rate-limited",
            Self::ProviderOverloaded => "provider overloaded",
            Self::Upstream5xx => "upstream 5xx",
            Self::Timeout => "timeout",
            Self::StalledStream => "stalled stream",
            Self::NetworkConnect => "connection failure",
            Self::NetworkReset => "connection reset",
            Self::Dns => "dns failure",
            Self::DecodeBody => "unreadable response body",
            Self::BridgeDropped => "bridge dropped stream",
            Self::ContextOverflow => "context overflow",
            Self::MalformedHistory => "malformed conversation state",
            Self::SessionExpired => "session expired",
            Self::AuthInvalid => "authentication failed",
            Self::QuotaExceeded => "quota exceeded",
            Self::BadRequest => "invalid request",
            Self::DegenerateOutput => "degenerate model output",
            Self::ResponseIncomplete => "response truncated",
            Self::ResponseCancelled => "response cancelled",
            Self::Unknown => "unknown upstream failure",
        }
    }

    pub(crate) fn recovery_action(self) -> RecoveryAction {
        match self {
            Self::RateLimited | Self::ProviderOverloaded => RecoveryAction::FailoverPreferred,
            Self::Upstream5xx
            | Self::Timeout
            | Self::StalledStream
            | Self::NetworkConnect
            | Self::NetworkReset
            | Self::Dns
            | Self::DecodeBody
            | Self::BridgeDropped => RecoveryAction::RetrySameProvider,
            Self::ContextOverflow => RecoveryAction::CompactContext,
            Self::MalformedHistory => RecoveryAction::RepairConversation,
            Self::SessionExpired | Self::AuthInvalid => RecoveryAction::Reauthenticate,
            Self::QuotaExceeded | Self::BadRequest | Self::DegenerateOutput | Self::Unknown => {
                RecoveryAction::Fatal
            }
            Self::ResponseIncomplete | Self::ResponseCancelled => RecoveryAction::RetrySameProvider,
        }
    }

    pub(crate) fn transient_kind(self) -> Option<TransientFailureKind> {
        match self {
            Self::RateLimited => Some(TransientFailureKind::RateLimited),
            Self::ProviderOverloaded => Some(TransientFailureKind::ProviderOverloaded),
            Self::Upstream5xx => Some(TransientFailureKind::Upstream5xx),
            Self::Timeout => Some(TransientFailureKind::Timeout),
            Self::StalledStream => Some(TransientFailureKind::StalledStream),
            Self::NetworkConnect => Some(TransientFailureKind::NetworkConnect),
            Self::NetworkReset => Some(TransientFailureKind::NetworkReset),
            Self::Dns => Some(TransientFailureKind::Dns),
            Self::DecodeBody => Some(TransientFailureKind::DecodeBody),
            Self::BridgeDropped => Some(TransientFailureKind::BridgeDropped),
            Self::ResponseIncomplete => Some(TransientFailureKind::ResponseIncomplete),
            Self::ResponseCancelled => Some(TransientFailureKind::ResponseCancelled),
            Self::ContextOverflow
            | Self::MalformedHistory
            | Self::SessionExpired
            | Self::AuthInvalid
            | Self::QuotaExceeded
            | Self::BadRequest
            | Self::DegenerateOutput
            | Self::Unknown => None,
        }
    }
}

impl TransientFailureKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::RateLimited => "rate-limited",
            Self::ProviderOverloaded => "provider overloaded",
            Self::Upstream5xx => "upstream 5xx",
            Self::Timeout => "timeout",
            Self::StalledStream => "stalled stream",
            Self::NetworkConnect => "connection failure",
            Self::NetworkReset => "connection reset",
            Self::Dns => "dns failure",
            Self::DecodeBody => "unreadable response body",
            Self::BridgeDropped => "bridge dropped stream",
            Self::ResponseIncomplete => "response truncated",
            Self::ResponseCancelled => "response cancelled",
        }
    }
}

impl TransientFailureKind {
    pub(crate) fn operator_detail(self, provider: &str, err_msg: &str) -> String {
        match self {
            Self::DecodeBody => format!("{provider} returned an unreadable response body"),
            Self::BridgeDropped => {
                format!("{provider} dropped the response stream before completion")
            }
            Self::NetworkReset => format!("connection to {provider} was reset mid-stream"),
            Self::NetworkConnect => format!("could not connect to {provider}"),
            Self::Dns => format!("could not resolve {provider} endpoint"),
            Self::Timeout => format!("{provider} did not respond before the timeout"),
            Self::StalledStream => format!("{provider} stream stopped producing output"),
            Self::ResponseIncomplete => {
                format!("{provider} truncated its response (output limit or content filter)")
            }
            Self::ResponseCancelled => format!("{provider} cancelled the response server-side"),
            _ => crate::util::truncate_str(err_msg, 300).to_string(),
        }
    }
}

fn upstream_failures_log_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".omegon").join("upstream-failures.jsonl")
}

pub(crate) fn append_upstream_failure_log(entry: &UpstreamFailureLogEntry) {
    use std::io::Write;

    let path = upstream_failures_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    let _ = writeln!(file, "{line}");
}

pub(crate) fn classify_upstream_error_for_provider(
    provider: &str,
    msg: &str,
) -> UpstreamErrorClass {
    let lower = msg.to_lowercase();
    if let Some(class) = apply_error_rules(PROVIDER_ERROR_RULES, Some(provider), &lower) {
        return class;
    }
    classify_upstream_error(msg)
}

pub(crate) fn classify_upstream_error(msg: &str) -> UpstreamErrorClass {
    let lower = msg.to_lowercase();

    if is_context_overflow(&lower) {
        return UpstreamErrorClass::ContextOverflow;
    }
    if is_malformed_history(&lower) {
        return UpstreamErrorClass::MalformedHistory;
    }
    apply_error_rules(GLOBAL_ERROR_RULES, None, &lower).unwrap_or(UpstreamErrorClass::Unknown)
}

/// Detect context-too-large errors that can be recovered by compaction.
/// Must NOT match general rate-limit 429s — those are transient and retried separately.
pub(crate) fn is_context_overflow(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("long context")
        || lower.contains("context length")
        || lower.contains("maximum context")
        || lower.contains("token limit")
        || lower.contains("request too large")
        || lower.contains("prompt is too long")
        || lower.contains("maximum number of tokens")
        || (lower.contains("extra usage") && lower.contains("context"))
}

/// Detect malformed request errors that can be recovered by stripping bad history.
/// These are 400-class errors from conversation structure issues.
pub(crate) fn is_malformed_history(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("tool_use_id")
        || lower.contains("tool_result")
        || lower.contains("thinking.signature")
        || lower.contains("content_block")
        || lower.contains("role must alternate")
        || lower.contains("must have a corresponding")
        || lower.contains("field required")
        || lower.contains("does not match pattern")
}

fn apply_error_rules(
    rules: &[ErrorRule],
    provider: Option<&str>,
    lower_msg: &str,
) -> Option<UpstreamErrorClass> {
    rules.iter().find_map(|rule| {
        let provider_matches = match provider {
            Some(provider) => rule.providers.contains(&provider),
            None => rule.providers.is_empty(),
        };
        if !provider_matches {
            return None;
        }
        let substring_match = rule
            .substrings
            .iter()
            .any(|needle| lower_msg.contains(needle));
        let word_match = rule
            .word_tokens
            .iter()
            .any(|token| contains_word(lower_msg, token));
        if substring_match || word_match {
            Some(rule.class)
        } else {
            None
        }
    })
}

pub(crate) fn classify_transient_error(msg: &str) -> Option<TransientFailureKind> {
    classify_upstream_error(msg).transient_kind()
}

pub(crate) fn is_transient_error(msg: &str) -> bool {
    classify_transient_error(msg).is_some()
}

/// Check if `text` contains `word` as a standalone token.
/// Word boundaries: spaces, punctuation, and start/end of string.
/// Hyphens and underscores are treated as word-joining (so "gpt-500" doesn't match "500").
pub(crate) fn contains_word(text: &str, word: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = text[start..].find(word) {
        let abs_pos = start + pos;
        let before_ok = abs_pos == 0 || !is_word_char(text.as_bytes()[abs_pos - 1]);
        let after_pos = abs_pos + word.len();
        let after_ok = after_pos >= text.len() || !is_word_char(text.as_bytes()[after_pos]);
        if before_ok && after_ok {
            return true;
        }
        start = abs_pos + 1;
    }
    false
}

/// Is this byte part of a "word" for boundary detection?
/// Alphanumeric plus hyphen and underscore (common in model names, versions).
fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_overflow_detection() {
        assert!(is_context_overflow(
            "This model's maximum context length is 128000 tokens"
        ));
        assert!(is_context_overflow("Request too large for model"));
        assert!(is_context_overflow("prompt is too long"));
        assert!(is_context_overflow("maximum number of tokens exceeded"));
        assert!(!is_context_overflow("rate limit exceeded"));
        assert!(!is_context_overflow("model not found"));
    }

    #[test]
    fn malformed_history_detection() {
        assert!(is_malformed_history("tool_use_id not found"));
        assert!(is_malformed_history(
            "role must alternate between user and assistant"
        ));
        assert!(is_malformed_history("thinking.signature is required"));
        assert!(!is_malformed_history("rate limit exceeded"));
    }

    #[test]
    fn classify_rate_limit() {
        assert_eq!(
            classify_upstream_error("429 too many requests"),
            UpstreamErrorClass::RateLimited,
        );
    }

    #[test]
    fn classify_overloaded() {
        assert_eq!(
            classify_upstream_error("the server is overloaded right now"),
            UpstreamErrorClass::ProviderOverloaded,
        );
    }

    #[test]
    fn classify_timeout() {
        assert_eq!(
            classify_upstream_error("request timed out after 30s"),
            UpstreamErrorClass::Timeout,
        );
    }

    #[test]
    fn classify_context_overflow() {
        assert_eq!(
            classify_upstream_error("maximum context length exceeded"),
            UpstreamErrorClass::ContextOverflow,
        );
    }

    #[test]
    fn classify_unknown_fallback() {
        assert_eq!(
            classify_upstream_error("something completely unexpected happened"),
            UpstreamErrorClass::Unknown,
        );
    }

    #[test]
    fn classify_degenerate_output() {
        assert_eq!(
            classify_upstream_error(
                "Model output degenerate: phrase \"append tests.\" repeated 30/40 recent chunks"
            ),
            UpstreamErrorClass::DegenerateOutput,
        );
    }

    #[test]
    fn transient_errors_detected() {
        assert!(is_transient_error("429 too many requests"));
        assert!(is_transient_error("502 bad gateway"));
        assert!(is_transient_error("connection timed out"));
        assert!(is_transient_error("server overloaded"));
        // NOT transient
        assert!(!is_transient_error("invalid api key"));
        assert!(!is_transient_error("quota exceeded"));
        assert!(!is_transient_error("model output degenerate"));
    }

    #[test]
    fn contains_word_boundaries() {
        assert!(contains_word("error 429 rate limited", "429"));
        assert!(contains_word("429 too many requests", "429"));
        assert!(contains_word("status: 429", "429"));
        // Should NOT match: status codes embedded in non-error contexts
        assert!(!contains_word("model gpt-500 not found", "500"));
    }

    #[test]
    fn recovery_actions_correct() {
        assert_eq!(
            UpstreamErrorClass::RateLimited.recovery_action(),
            RecoveryAction::FailoverPreferred,
        );
        assert_eq!(
            UpstreamErrorClass::ContextOverflow.recovery_action(),
            RecoveryAction::CompactContext,
        );
        assert_eq!(
            UpstreamErrorClass::MalformedHistory.recovery_action(),
            RecoveryAction::RepairConversation,
        );
        assert_eq!(
            UpstreamErrorClass::DegenerateOutput.recovery_action(),
            RecoveryAction::Fatal,
        );
        assert_eq!(
            UpstreamErrorClass::Timeout.recovery_action(),
            RecoveryAction::RetrySameProvider,
        );
    }

    #[test]
    fn classify_response_incomplete() {
        assert_eq!(
            classify_upstream_error_for_provider(
                "openai-codex",
                "Codex: response incomplete (max_tokens) — output was truncated",
            ),
            UpstreamErrorClass::ResponseIncomplete,
        );
        // Must be transient so the retry loop fires
        assert!(
            UpstreamErrorClass::ResponseIncomplete
                .transient_kind()
                .is_some()
        );
        assert_eq!(
            UpstreamErrorClass::ResponseIncomplete.recovery_action(),
            RecoveryAction::RetrySameProvider,
        );
    }

    #[test]
    fn classify_response_cancelled() {
        assert_eq!(
            classify_upstream_error_for_provider(
                "openai-codex",
                "Codex: response cancelled by server",
            ),
            UpstreamErrorClass::ResponseCancelled,
        );
        assert!(
            UpstreamErrorClass::ResponseCancelled
                .transient_kind()
                .is_some()
        );
    }

    #[test]
    fn classify_stream_closed_without_completion() {
        assert_eq!(
            classify_upstream_error(
                "Codex: stream closed without completion (had 1200b text, 0 tool calls)",
            ),
            UpstreamErrorClass::BridgeDropped,
        );
        // BridgeDropped is transient → retry
        assert!(UpstreamErrorClass::BridgeDropped.transient_kind().is_some());
    }

    #[test]
    fn provider_specific_override() {
        // Anthropic overloaded_error → ProviderOverloaded, not generic
        assert_eq!(
            classify_upstream_error_for_provider("anthropic", "overloaded_error"),
            UpstreamErrorClass::ProviderOverloaded,
        );
    }

    #[test]
    fn classify_bare_520_as_transient_upstream_failure() {
        assert_eq!(
            classify_upstream_error_for_provider("openai-codex", "Codex 520: error code: 520"),
            UpstreamErrorClass::Upstream5xx,
        );
        assert_eq!(
            UpstreamErrorClass::Upstream5xx.transient_kind(),
            Some(TransientFailureKind::Upstream5xx),
        );
    }

    #[test]
    fn classify_edge_proxy_52x_and_gateway_failures_as_upstream_5xx() {
        assert_eq!(
            classify_upstream_error("504 Gateway Timeout"),
            UpstreamErrorClass::Upstream5xx,
        );
        assert_eq!(
            classify_upstream_error("Error 522: origin unreachable"),
            UpstreamErrorClass::Upstream5xx,
        );
        assert_eq!(
            classify_upstream_error("upstream connect error or disconnect/reset before headers"),
            UpstreamErrorClass::Upstream5xx,
        );
    }

    #[test]
    fn classify_codex_401_scope_message_as_auth_state_not_scope_failure() {
        assert_eq!(
            classify_upstream_error_for_provider(
                "openai-codex",
                "Codex 401: You have insufficient permissions for this operation. Missing scopes: api.responses.write",
            ),
            UpstreamErrorClass::AuthInvalid,
        );
    }

    #[test]
    fn classify_codex_401_expired_token_as_session_expired() {
        assert_eq!(
            classify_upstream_error_for_provider(
                "openai-codex",
                "Codex 401 Unauthorized: token expired, please log in again",
            ),
            UpstreamErrorClass::SessionExpired,
        );
    }

    #[test]
    fn classify_rate_limit_variants() {
        assert_eq!(
            classify_upstream_error("retry-after: 30; request limit reached for organization"),
            UpstreamErrorClass::RateLimited,
        );
        assert_eq!(
            classify_upstream_error("tokens per min exceeded (tpm)"),
            UpstreamErrorClass::RateLimited,
        );
    }

    #[test]
    fn classify_capacity_variants_as_provider_overloaded() {
        assert_eq!(
            classify_upstream_error("Selected model is at capacity"),
            UpstreamErrorClass::ProviderOverloaded,
        );
        assert_eq!(
            classify_upstream_error("server is busy due to high demand"),
            UpstreamErrorClass::ProviderOverloaded,
        );
    }

    #[test]
    fn log_entry_serializes() {
        let entry = UpstreamFailureLogEntry {
            timestamp: "2026-04-03T00:00:00Z".into(),
            provider: "openai".into(),
            model: "gpt-4.1".into(),
            failure_kind: "rate-limited".into(),
            internal_class: "RateLimited".into(),
            recovery_action: RecoveryAction::FailoverPreferred,
            attempt: 1,
            delay_ms: 750,
            message: "429 too many requests".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"provider\":\"openai\""));
        assert!(json.contains("\"recovery_action\":\"failover_preferred\""));
    }
}
