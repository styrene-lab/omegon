use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_GITHUB_API_BASE_URL: &str = "https://api.github.com";
const DEFAULT_COPILOT_API_BASE_URL: &str = "https://api.business.githubcopilot.com";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubCopilotContractProbe {
    pub sign_in_source: String,
    pub token_exchange: GithubCopilotTokenExchangeProbe,
    pub models: Option<GithubCopilotModelsProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubCopilotTokenExchangeProbe {
    pub status: u16,
    pub success: bool,
    pub json_keys: Vec<String>,
    pub token_present: bool,
    pub expires_at: Option<i64>,
    pub refresh_in: Option<i64>,
    pub endpoints: Vec<String>,
    pub redacted_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubCopilotModelsProbe {
    pub base_url: String,
    pub status: u16,
    pub success: bool,
    pub model_ids: Vec<String>,
    pub redacted_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGithubSignin {
    pub source: GithubSigninSource,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubSigninSource {
    OmegonGithubCopilot,
    OmegonGithub,
    GithubCliDiagnostic,
    Environment,
}

impl GithubSigninSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OmegonGithubCopilot => "omegon:github-copilot",
            Self::OmegonGithub => "omegon:github",
            Self::GithubCliDiagnostic => "gh-diagnostic",
            Self::Environment => "environment",
        }
    }
}

pub async fn resolve_github_signin() -> anyhow::Result<ResolvedGithubSignin> {
    if let Some((token, _)) = crate::providers::resolve_api_key_sync("github-copilot") {
        if !token.trim().is_empty() {
            return Ok(ResolvedGithubSignin {
                source: GithubSigninSource::OmegonGithubCopilot,
                token,
            });
        }
    }
    if let Some((token, _)) = crate::providers::resolve_api_key_sync("github") {
        if !token.trim().is_empty() {
            return Ok(ResolvedGithubSignin {
                source: GithubSigninSource::OmegonGithub,
                token,
            });
        }
    }
    if let Some(token) = github_cli_token().await {
        return Ok(ResolvedGithubSignin {
            source: GithubSigninSource::GithubCliDiagnostic,
            token,
        });
    }
    for name in ["GITHUB_TOKEN", "GITHUB_COPILOT_TOKEN", "COPILOT_OAUTH_TOKEN"] {
        if let Ok(token) = std::env::var(name) {
            if !token.trim().is_empty() {
                return Ok(ResolvedGithubSignin {
                    source: GithubSigninSource::Environment,
                    token,
                });
            }
        }
    }
    anyhow::bail!(
        "No GitHub sign-in found for GitHub Copilot. Checked Omegon github-copilot login, Omegon github login, GitHub CLI session, and environment fallback. Run `omegon auth login github-copilot` or `gh auth login`."
    )
}

async fn github_cli_token() -> Option<String> {
    let output = tokio::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!token.is_empty()).then_some(token)
}

pub async fn probe_github_copilot_contract() -> anyhow::Result<GithubCopilotContractProbe> {
    let signin = resolve_github_signin().await?;
    probe_github_copilot_contract_with_token_and_source(&signin.token, signin.source.as_str()).await
}

pub async fn probe_github_copilot_contract_with_token(
    github_token: &str,
) -> anyhow::Result<GithubCopilotContractProbe> {
    probe_github_copilot_contract_with_token_and_source(github_token, "explicit").await
}

pub async fn probe_github_copilot_contract_with_token_and_source(
    github_token: &str,
    sign_in_source: &str,
) -> anyhow::Result<GithubCopilotContractProbe> {
    let client = reqwest::Client::new();
    let github_api_base_url = std::env::var("GITHUB_API_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_GITHUB_API_BASE_URL.to_string());
    let copilot_api_base_url = std::env::var("GITHUB_COPILOT_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_COPILOT_API_BASE_URL.to_string());

    let token_url = format!(
        "{}/copilot_internal/v2/token",
        github_api_base_url.trim_end_matches('/')
    );
    let response = client
        .get(token_url)
        .header("Authorization", format!("Bearer {github_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "omegon-github-copilot-probe")
        .header("Editor-Version", "omegon/0")
        .header("Editor-Plugin-Version", "omegon/0")
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&body).ok();
    let token = parsed
        .as_ref()
        .and_then(|value| value.get("token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    let token_exchange = summarize_token_exchange(status.as_u16(), parsed.as_ref(), &body);

    let models = if status.is_success() {
        if let Some(copilot_token) = token {
            Some(probe_models(&client, &copilot_api_base_url, &copilot_token).await?)
        } else {
            None
        }
    } else {
        None
    };

    Ok(GithubCopilotContractProbe {
        sign_in_source: sign_in_source.to_string(),
        token_exchange,
        models,
    })
}

async fn probe_models(
    client: &reqwest::Client,
    base_url: &str,
    copilot_token: &str,
) -> anyhow::Result<GithubCopilotModelsProbe> {
    let models_url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = client
        .get(models_url)
        .header("Authorization", format!("Bearer {copilot_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "omegon-github-copilot-probe")
        .header("Editor-Version", "omegon/0")
        .header("Editor-Plugin-Version", "omegon/0")
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&body).ok();
    Ok(GithubCopilotModelsProbe {
        base_url: base_url.to_string(),
        status: status.as_u16(),
        success: status.is_success(),
        model_ids: parsed.as_ref().map(extract_model_ids).unwrap_or_default(),
        redacted_error: if status.is_success() {
            None
        } else {
            Some(redact_probe_body(&body))
        },
    })
}

fn summarize_token_exchange(
    status: u16,
    parsed: Option<&Value>,
    raw_body: &str,
) -> GithubCopilotTokenExchangeProbe {
    let json_keys = parsed
        .and_then(Value::as_object)
        .map(|object| {
            let mut keys: Vec<String> = object.keys().cloned().collect();
            keys.sort();
            keys
        })
        .unwrap_or_default();
    GithubCopilotTokenExchangeProbe {
        status,
        success: (200..300).contains(&status),
        json_keys,
        token_present: parsed
            .and_then(|value| value.get("token"))
            .and_then(Value::as_str)
            .is_some_and(|token| !token.is_empty()),
        expires_at: parsed
            .and_then(|value| value.get("expires_at"))
            .and_then(Value::as_i64),
        refresh_in: parsed
            .and_then(|value| value.get("refresh_in"))
            .and_then(Value::as_i64),
        endpoints: parsed.map(extract_endpoint_values).unwrap_or_default(),
        redacted_error: if (200..300).contains(&status) {
            None
        } else {
            Some(redact_probe_body(raw_body))
        },
    }
}

fn extract_endpoint_values(value: &Value) -> Vec<String> {
    let mut endpoints = Vec::new();
    collect_endpoint_values(value, &mut endpoints);
    endpoints.sort();
    endpoints.dedup();
    endpoints
}

fn collect_endpoint_values(value: &Value, endpoints: &mut Vec<String>) {
    match value {
        Value::String(text)
            if text.starts_with("https://api.githubcopilot.com")
                || text.starts_with("https://api.business.githubcopilot.com") =>
        {
            endpoints.push(text.to_string());
        }
        Value::Array(items) => {
            for item in items {
                collect_endpoint_values(item, endpoints);
            }
        }
        Value::Object(object) => {
            for value in object.values() {
                collect_endpoint_values(value, endpoints);
            }
        }
        _ => {}
    }
}

fn extract_model_ids(value: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    collect_model_ids(value, &mut ids);
    ids.sort();
    ids.dedup();
    ids
}

fn collect_model_ids(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                ids.push(id.to_string());
            }
            if let Some(id) = object.get("model_id").and_then(Value::as_str) {
                ids.push(id.to_string());
            }
            for value in object.values() {
                collect_model_ids(value, ids);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_model_ids(item, ids);
            }
        }
        _ => {}
    }
}

fn redact_probe_body(body: &str) -> String {
    let truncated: String = body.chars().take(500).collect();
    let mut out = String::with_capacity(truncated.len());
    let mut in_long_token = false;
    let mut token_len = 0usize;
    for ch in truncated.chars() {
        let tokenish = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '~' | '+' | '/');
        if tokenish {
            token_len += 1;
            if token_len > 20 {
                if !in_long_token {
                    out.push_str("<redacted>");
                    in_long_token = true;
                }
                continue;
            }
            if !in_long_token {
                out.push(ch);
            }
        } else {
            in_long_token = false;
            token_len = 0;
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn github_signin_source_names_are_operator_oriented() {
        assert_eq!(GithubSigninSource::OmegonGithubCopilot.as_str(), "omegon:github-copilot");
        assert_eq!(GithubSigninSource::OmegonGithub.as_str(), "omegon:github");
        assert_eq!(GithubSigninSource::GithubCliDiagnostic.as_str(), "gh-diagnostic");
        assert_eq!(GithubSigninSource::Environment.as_str(), "environment");
    }

    #[test]
    fn token_exchange_summary_redacts_token_value() {
        let value = json!({
            "token": "ghu_this_token_must_not_appear_in_output",
            "expires_at": 123,
            "refresh_in": 60,
            "endpoints": { "api": "https://api.githubcopilot.com" }
        });
        let summary = summarize_token_exchange(200, Some(&value), "");
        assert!(summary.token_present);
        assert_eq!(summary.expires_at, Some(123));
        assert!(summary.json_keys.contains(&"token".to_string()));
        assert!(summary.endpoints.contains(&"https://api.githubcopilot.com".to_string()));
    }

    #[test]
    fn model_ids_are_extracted_from_nested_shapes() {
        let value = json!({
            "data": [
                {"id": "gpt-5.5"},
                {"model_id": "claude-sonnet-4.6"}
            ]
        });
        let ids = extract_model_ids(&value);
        assert_eq!(ids, vec!["claude-sonnet-4.6".to_string(), "gpt-5.5".to_string()]);
    }

    #[test]
    fn redaction_removes_long_tokenish_runs() {
        let redacted = redact_probe_body("error token abcdefghijklmnopqrstuvwxyz1234567890 done");
        assert!(redacted.contains("<redacted>"));
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz1234567890"));
    }
}
