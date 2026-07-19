pub(super) fn compact_model_label(name: &str, provider: &str) -> String {
    let suffix = compact_provider_suffix(provider);
    if suffix.is_empty() || model_name_already_mentions_provider(name, suffix) {
        name.to_string()
    } else {
        format!("{name} ({suffix})")
    }
}

fn compact_provider_suffix(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "Claude",
        "openai" => "OpenAI",
        "openai-codex" => "Codex",
        "google" => "Gemini",
        "google-antigravity" => "Antigravity",
        "ollama-cloud" => "Ollama Cloud",
        "openrouter" => "OpenRouter",
        "groq" => "Groq",
        "xai" => "xAI",
        "mistral" => "Mistral",
        "cerebras" => "Cerebras",
        "moonshot" => "Moonshot",
        "opencode-go" => "OpenCode",
        "perplexity" => "Perplexity",
        _ => "",
    }
}

fn model_name_already_mentions_provider(name: &str, suffix: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let suffix = suffix.to_ascii_lowercase();
    name.contains(&suffix)
        || (suffix == "claude" && name.contains("claude"))
        || (suffix == "gemini" && name.contains("gemini"))
        || (suffix == "openai" && name.contains("gpt"))
}

pub(super) fn acp_model_provider_available(provider_id: &str) -> bool {
    if matches!(provider_id, "ollama") {
        return true;
    }
    let Some(provider) = crate::auth::provider_by_id(provider_id) else {
        return false;
    };
    if crate::auth::provider_session_status(provider)
        == crate::auth::ProviderSessionStatus::Configured
    {
        return true;
    }
    crate::auth::read_external_credentials(provider.auth_key)
        .is_some_and(|creds| !creds.access.trim().is_empty() && !creds.is_expired())
}

pub(super) fn unavailable_current_model_label(current_model: &str) -> String {
    format!("{current_model} (current, unavailable)")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_model_label_uses_short_provider_suffixes() {
        assert_eq!(
            compact_model_label("GPT-5.5", "openai-codex"),
            "GPT-5.5 (Codex)"
        );
        assert_eq!(compact_model_label("GPT-5.5", "openai"), "GPT-5.5");
        assert_eq!(
            compact_model_label("Claude Opus 4.7", "anthropic"),
            "Claude Opus 4.7"
        );
        assert_eq!(
            compact_model_label("Qwen3 Coder 480B", "ollama-cloud"),
            "Qwen3 Coder 480B (Ollama Cloud)"
        );
    }
}
