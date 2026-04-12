//! HTTP plugin Feature — wraps a plugin manifest as a Feature with HTTP-backed
//! tool execution, context injection, and event forwarding.

use async_trait::async_trait;
use omegon_traits::*;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

use super::manifest::{PluginManifest, resolve_template};

/// A Feature backed by HTTP endpoints declared in a plugin manifest.
pub struct HttpPluginFeature {
    manifest: PluginManifest,
    client: reqwest::Client,
}

impl HttpPluginFeature {
    pub fn new(manifest: PluginManifest) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("omegon-plugin/1.0")
            .build()
            .unwrap_or_default();
        Self { manifest, client }
    }

    /// Resolve template variables from env + tool args.
    fn resolve_url(&self, template: &str, args: &Value) -> String {
        let mut vars = HashMap::new();
        if let Some(obj) = args.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    vars.insert(k.clone(), s.to_string());
                } else {
                    vars.insert(k.clone(), v.to_string());
                }
            }
        }
        resolve_template(template, &vars)
    }

    /// Fire-and-forget event POST (best-effort, no error propagation).
    async fn post_event(&self, endpoint: &str, payload: &Value) {
        let url = resolve_template(endpoint, &HashMap::new());
        match self
            .client
            .post(&url)
            .json(payload)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) if !resp.status().is_success() => {
                tracing::debug!(plugin = %self.manifest.plugin.name, status = %resp.status(), "plugin event POST failed");
            }
            Err(e) => {
                tracing::debug!(plugin = %self.manifest.plugin.name, error = %e, "plugin event POST error");
            }
            _ => {}
        }
    }
}

#[async_trait]
impl Feature for HttpPluginFeature {
    fn name(&self) -> &str {
        &self.manifest.plugin.name
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.manifest
            .tools
            .iter()
            .map(|t| ToolDefinition {
                name: t.name.clone(),
                label: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            })
            .collect()
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let tool = self
            .manifest
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| anyhow::anyhow!("unknown plugin tool: {tool_name}"))?;

        let url = self.resolve_url(&tool.endpoint, &args);
        let method =
            tool.method
                .as_deref()
                .unwrap_or(if args.as_object().is_some_and(|o| !o.is_empty()) {
                    "POST"
                } else {
                    "GET"
                });
        let timeout = Duration::from_secs(tool.timeout_secs);

        let resp = match method.to_uppercase().as_str() {
            "GET" => self.client.get(&url).timeout(timeout).send().await,
            "POST" => {
                self.client
                    .post(&url)
                    .json(&args)
                    .timeout(timeout)
                    .send()
                    .await
            }
            "PUT" => {
                self.client
                    .put(&url)
                    .json(&args)
                    .timeout(timeout)
                    .send()
                    .await
            }
            "DELETE" => self.client.delete(&url).timeout(timeout).send().await,
            other => anyhow::bail!("unsupported HTTP method: {other}"),
        };

        match resp {
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                if status.is_success() {
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: body }],
                        details: serde_json::json!({ "status": status.as_u16() }),
                    })
                } else {
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("Plugin HTTP error {status}: {body}"),
                        }],
                        details: serde_json::json!({ "status": status.as_u16(), "error": true }),
                    })
                }
            }
            Err(e) => {
                // Graceful degradation — return error as text, don't crash
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Plugin {} unreachable: {e}", self.manifest.plugin.name),
                    }],
                    details: serde_json::json!({ "error": true }),
                })
            }
        }
    }

    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        let ctx_config = self.manifest.context.as_ref()?;

        // Try local file first (fast, no network)
        if let Some(ref local_file) = ctx_config.local_file {
            let cwd = std::env::current_dir().ok()?;
            // Search up from cwd
            let mut dir = cwd;
            for _ in 0..6 {
                let path = dir.join(local_file);
                if path.exists()
                    && let Ok(content) = std::fs::read_to_string(&path)
                    && !content.trim().is_empty()
                {
                    return Some(ContextInjection {
                        source: format!("plugin:{}", self.manifest.plugin.name),
                        content,
                        ttl_turns: ctx_config.ttl_turns,
                        priority: ctx_config.priority as u8,
                    });
                }
                if !dir.pop() {
                    break;
                }
            }
        }

        // HTTP context injection is async but provide_context is sync.
        // For HTTP-based context, we'd need to pre-fetch on SessionStart.
        // For now, only local file context is supported in provide_context.
        // HTTP context is fetched via on_event(SessionStart) → cached.
        None
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        let events = match &self.manifest.events {
            Some(e) => e,
            None => return vec![],
        };

        match event {
            BusEvent::TurnEnd { turn, .. } => {
                if let Some(ref endpoint) = events.turn_end {
                    let client = self.client.clone();
                    let url = resolve_template(endpoint, &HashMap::new());
                    let payload = serde_json::json!({ "event": "turn_end", "turn": turn });
                    // Best-effort plugin telemetry hook — delivery is not guaranteed.
                    crate::task_spawn::spawn_best_effort_result(
                        "http-plugin-turn-end",
                        async move {
                            let _ = client
                                .post(&url)
                                .json(&payload)
                                .timeout(Duration::from_secs(5))
                                .send()
                                .await;
                            Ok(())
                        },
                    );
                }
            }
            BusEvent::SessionStart { .. } => {
                if let Some(ref endpoint) = events.session_start {
                    let client = self.client.clone();
                    let url = resolve_template(endpoint, &HashMap::new());
                    let payload = serde_json::json!({ "event": "session_start" });
                    // Best-effort plugin telemetry hook — delivery is not guaranteed.
                    crate::task_spawn::spawn_best_effort_result(
                        "http-plugin-session-start",
                        async move {
                            let _ = client
                                .post(&url)
                                .json(&payload)
                                .timeout(Duration::from_secs(5))
                                .send()
                                .await;
                            Ok(())
                        },
                    );
                }
            }
            _ => {}
        }

        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::manifest::*;

    fn test_manifest() -> PluginManifest {
        toml::from_str(
            r#"
            [plugin]
            name = "test-plugin"

            [[tools]]
            name = "test_tool"
            description = "A test tool"
            endpoint = "http://localhost:9999/api/test"
        "#,
        )
        .unwrap()
    }

    #[test]
    fn feature_name_from_manifest() {
        let feature = HttpPluginFeature::new(test_manifest());
        assert_eq!(feature.name(), "test-plugin");
    }

    #[test]
    fn tools_from_manifest() {
        let feature = HttpPluginFeature::new(test_manifest());
        let tools = feature.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test_tool");
        assert_eq!(tools[0].description, "A test tool");
    }

    #[tokio::test]
    async fn execute_graceful_degradation() {
        // Tool call to unreachable endpoint should return error text, not crash
        let mut manifest = test_manifest();
        manifest.tools[0].timeout_secs = 1;
        let feature = HttpPluginFeature::new(manifest);
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute("test_tool", "tc1", serde_json::json!({}), cancel)
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("unreachable") || text.contains("error"),
            "should gracefully degrade: {text}"
        );
    }

    #[test]
    fn context_from_local_file() {
        let dir = tempfile::tempdir().unwrap();
        let scribe_file = dir.path().join(".scribe");
        std::fs::write(
            &scribe_file,
            "partnership: acme\nengagement: widget-rewrite",
        )
        .unwrap();

        let manifest: PluginManifest = toml::from_str(
            r#"
            [plugin]
            name = "test"

            [context]
            local_file = ".scribe"
            ttl_turns = 15
            priority = 30
        "#,
        )
        .unwrap();

        let feature = HttpPluginFeature::new(manifest);
        // Set cwd to the temp dir so the local file is found
        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let signals = ContextSignals {
            user_prompt: "",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 200_000,
        };
        let ctx = feature.provide_context(&signals);
        assert!(ctx.is_some(), "should inject context from .scribe file");
        let injection = ctx.unwrap();
        assert!(injection.content.contains("partnership: acme"));
        assert_eq!(injection.ttl_turns, 15);

        std::env::set_current_dir(old_cwd).unwrap();
    }
}
