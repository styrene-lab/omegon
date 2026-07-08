use omegon_traits::SlashCommandResponse;

pub async fn secrets_view_response(
    secrets: &omegon_secrets::SecretsManager,
) -> SlashCommandResponse {
    let names = secrets.list_recipes();
    let mut out = String::new();
    if names.is_empty() {
        out.push_str("No secrets stored.\n");
    } else {
        out.push_str(&format!("🔐 Secrets ({})\n\n", names.len()));
        for (name, recipe) in &names {
            out.push_str(&format!("  {name:<24} {recipe}\n"));
        }
        out.push('\n');
    }
    out.push_str("Common first-party secrets:\n");
    out.push_str("  /secrets set BRAVE_API_KEY          hidden input prompt for web search\n");
    out.push_str("  /secrets set TAVILY_API_KEY         hidden input prompt for web search\n");
    out.push_str("  /secrets set SERPER_API_KEY         hidden input prompt for web search\n");
    out.push_str("  /secrets set FIRECRAWL_API_KEY      hidden input prompt for web search\n");
    out.push_str(
        "  /secrets set ANTHROPIC_API_KEY      hidden input prompt for provider API key\n",
    );
    out.push_str(
        "  /secrets set OPENAI_API_KEY         hidden input prompt for provider API key\n",
    );
    out.push_str(
        "  /secrets set OPENROUTER_API_KEY     hidden input prompt for provider API key\n",
    );
    out.push_str("  /secrets set GITHUB_TOKEN cmd:gh auth token    always fresh from CLI\n");
    out.push_str("  /secrets set GH_TOKEN cmd:gh auth token        GitHub CLI-compatible alias\n");
    out.push_str(
        "  /secrets set GITLAB_TOKEN           hidden input prompt for GitLab forge workflows\n\n",
    );
    out.push_str("Other recipe examples:\n");
    out.push_str("  /secrets set NPM_TOKEN cmd:npm token get       always fresh from CLI\n");
    out.push_str(
        "  /secrets set AWS_SECRET_ACCESS_KEY env:AWS_SECRET_ACCESS_KEY  from environment\n\n",
    );
    out.push_str("Check or clear local binding:\n");
    out.push_str("  /secrets get GITHUB_TOKEN       checks resolution, never prints value\n");
    out.push_str("  /secrets delete GITHUB_TOKEN    clears local value/recipe binding");
    SlashCommandResponse {
        accepted: true,
        output: Some(out),
    }
}

pub async fn secrets_set_response(
    secrets: &omegon_secrets::SecretsManager,
    name: &str,
    value: &str,
) -> SlashCommandResponse {
    let is_recipe = value.contains(':')
        && ["env:", "cmd:", "vault:", "keyring:", "file:"]
            .iter()
            .any(|p| value.starts_with(p));
    let result = if is_recipe {
        secrets.set_recipe(name, value)
    } else {
        secrets.set_keyring_secret(name, value)
    };
    match result {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(if is_recipe {
                format!(
                    "✓ Secret recipe '{name}' stored as {value}.\n  Values resolved from the recipe are redacted from output."
                )
            } else {
                format!(
                    "✓ Secret '{name}' stored (encrypted in OS keyring).\n  The agent will redact this value from all output."
                )
            }),
        },
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Error storing secret: {e}")),
        },
    }
}

pub async fn secrets_get_response(
    secrets: &omegon_secrets::SecretsManager,
    name: &str,
) -> SlashCommandResponse {
    match secrets.resolve(name) {
        Some(_) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "🔒 Secret '{name}' resolves successfully. Values are never printed."
            )),
        },
        None => SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Secret '{name}' not found.\n  Use /secrets for the known inventory and /secrets status for stored bindings."
            )),
        },
    }
}

pub async fn secrets_delete_response(
    secrets: &omegon_secrets::SecretsManager,
    name: &str,
) -> SlashCommandResponse {
    match secrets.delete_recipe(name) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "✓ Secret '{name}' local binding cleared. Declared capability requirements remain visible."
            )),
        },
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Error: {e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn secrets_view_response_lists_first_party_catalog_examples() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets = omegon_secrets::SecretsManager::new(tmp.path()).unwrap();

        let response = secrets_view_response(&secrets).await;

        assert!(response.accepted);
        let output = response.output.unwrap_or_default();
        for expected in [
            "BRAVE_API_KEY",
            "TAVILY_API_KEY",
            "SERPER_API_KEY",
            "FIRECRAWL_API_KEY",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "OPENROUTER_API_KEY",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "GITLAB_TOKEN",
        ] {
            assert!(output.contains(expected), "missing {expected} in {output}");
        }
        assert!(!output.contains("OPENROUTER_KEY"), "{output}");
        assert!(!output.contains("AWS_SECRET env:"), "{output}");
    }

    #[tokio::test]
    async fn secrets_get_missing_points_to_inventory_and_status() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets = omegon_secrets::SecretsManager::new(tmp.path()).unwrap();

        let response = secrets_get_response(&secrets, "BRAVE_API_KEY").await;

        assert!(!response.accepted);
        let output = response.output.unwrap_or_default();
        assert!(
            output.contains("Use /secrets for the known inventory"),
            "{output}"
        );
        assert!(output.contains("/secrets status"), "{output}");
    }

    #[tokio::test]
    async fn secrets_delete_response_says_binding_cleared() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets = omegon_secrets::SecretsManager::new(tmp.path()).unwrap();
        secrets
            .set_recipe("GITHUB_TOKEN", "env:GITHUB_TOKEN")
            .unwrap();

        let response = secrets_delete_response(&secrets, "GITHUB_TOKEN").await;

        assert!(response.accepted);
        let output = response.output.unwrap_or_default();
        assert!(output.contains("local binding cleared"), "{output}");
        assert!(output.contains("requirements remain visible"), "{output}");
        assert!(secrets.list_recipes().is_empty());
    }

    #[tokio::test]
    async fn secrets_get_response_never_prints_value() {
        let _guard = crate::test_support::env::lock_async().await;
        let tmp = tempfile::tempdir().unwrap();
        let secrets = omegon_secrets::SecretsManager::new(tmp.path()).unwrap();
        secrets.set_recipe("TEST_TOKEN", "env:TEST_TOKEN").unwrap();
        unsafe { std::env::set_var("TEST_TOKEN", "super-secret-value") };

        let response = secrets_get_response(&secrets, "TEST_TOKEN").await;

        unsafe { std::env::remove_var("TEST_TOKEN") };
        assert!(response.accepted);
        let output = response.output.unwrap_or_default();
        assert!(!output.contains("super-secret-value"), "{output}");
        assert!(output.contains("never printed"), "{output}");
    }
}
