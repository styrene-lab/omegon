//! Plugin manifest — TOML schema for external plugin declarations.

use serde::Deserialize;
use std::collections::HashMap;

/// Top-level plugin manifest (plugin.toml).
#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub activation: Activation,
    #[serde(default)]
    pub context: Option<ContextConfig>,
    #[serde(default)]
    pub tools: Vec<ToolConfig>,
    #[serde(default)]
    pub events: Option<EventConfig>,
}

#[derive(Debug, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Activation rules — when should this plugin load?
#[derive(Debug, Default, Deserialize)]
pub struct Activation {
    /// Plugin activates if any of these files exist (relative to cwd)
    #[serde(default)]
    pub marker_files: Vec<String>,
    /// Plugin activates if any of these env vars are set
    #[serde(default)]
    pub env_vars: Vec<String>,
    /// Always activate (ignores marker_files and env_vars)
    #[serde(default)]
    pub always: bool,
}

impl Activation {
    /// Check if the plugin should activate given the current working directory.
    pub fn is_active(&self, cwd: &std::path::Path) -> bool {
        if self.always {
            return true;
        }
        // Check marker files
        for marker in &self.marker_files {
            // Search cwd and up to 5 parents
            let mut dir = cwd.to_path_buf();
            for _ in 0..6 {
                if dir.join(marker).exists() {
                    return true;
                }
                if !dir.pop() { break; }
            }
        }
        // Check env vars
        for var in &self.env_vars {
            if std::env::var(var).is_ok() {
                return true;
            }
        }
        false
    }
}

/// Context injection config — enrich the agent's system prompt.
#[derive(Debug, Deserialize)]
pub struct ContextConfig {
    /// HTTP endpoint to call for context (GET, returns plain text)
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Local file to read for context (relative to cwd)
    #[serde(default)]
    pub local_file: Option<String>,
    /// TTL in turns (how long the injected context stays active)
    #[serde(default = "default_ttl")]
    pub ttl_turns: u32,
    /// Priority (higher = injected earlier in the prompt)
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_ttl() -> u32 { 20 }
fn default_priority() -> u32 { 40 }

/// Tool declaration — backed by an HTTP endpoint.
#[derive(Debug, Deserialize)]
pub struct ToolConfig {
    pub name: String,
    pub description: String,
    /// HTTP endpoint to call when the tool is invoked.
    /// Supports `{var}` template substitution from env vars and tool args.
    pub endpoint: String,
    /// HTTP method (default: POST for tools with parameters, GET otherwise)
    #[serde(default)]
    pub method: Option<String>,
    /// JSON Schema for tool parameters
    #[serde(default = "default_parameters")]
    pub parameters: serde_json::Value,
    /// Timeout in seconds (default: 10)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_parameters() -> serde_json::Value {
    serde_json::json!({"type": "object", "properties": {}})
}
fn default_timeout() -> u64 { 10 }

/// Event forwarding config — POST agent events to external endpoints.
#[derive(Debug, Default, Deserialize)]
pub struct EventConfig {
    /// POST to this endpoint on TurnEnd
    #[serde(default)]
    pub turn_end: Option<String>,
    /// POST to this endpoint on SessionStart
    #[serde(default)]
    pub session_start: Option<String>,
    /// POST to this endpoint on AgentEnd
    #[serde(default)]
    pub agent_end: Option<String>,
}

/// Resolve `{VAR}` template variables in a URL string.
/// Checks env vars first, then falls back to the provided args map.
pub fn resolve_template(template: &str, args: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    // Find all {VAR} patterns
    while let Some(start) = result.find('{') {
        if let Some(end) = result[start..].find('}') {
            let var = &result[start + 1..start + end];
            let replacement = std::env::var(var)
                .ok()
                .or_else(|| args.get(var).cloned())
                .unwrap_or_default();
            result = format!("{}{}{}", &result[..start], replacement, &result[start + end + 1..]);
        } else {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_manifest() {
        let toml_str = r#"
            [plugin]
            name = "test"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.plugin.name, "test");
        assert!(manifest.tools.is_empty());
        assert!(manifest.context.is_none());
    }

    #[test]
    fn parse_full_manifest() {
        let toml_str = r#"
            [plugin]
            name = "scribe"
            version = "0.1.0"
            description = "Engagement tracking"

            [activation]
            marker_files = [".scribe"]
            env_vars = ["SCRIBE_URL"]

            [context]
            endpoint = "{SCRIBE_URL}/api/context"
            ttl_turns = 20
            priority = 40

            [[tools]]
            name = "scribe_status"
            description = "Get engagement status"
            endpoint = "{SCRIBE_URL}/api/status"
            method = "GET"

            [[tools]]
            name = "scribe_log"
            description = "Add work log entry"
            endpoint = "{SCRIBE_URL}/api/logs"
            parameters = { type = "object", properties = { content = { type = "string" } }, required = ["content"] }

            [events]
            turn_end = "{SCRIBE_URL}/api/sessions/ingest"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.plugin.name, "scribe");
        assert_eq!(manifest.tools.len(), 2);
        assert_eq!(manifest.tools[0].name, "scribe_status");
        assert!(manifest.context.is_some());
        assert!(manifest.events.as_ref().unwrap().turn_end.is_some());
    }

    #[test]
    fn activation_marker_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".scribe"), "").unwrap();

        let activation = Activation {
            marker_files: vec![".scribe".into()],
            ..Default::default()
        };
        assert!(activation.is_active(dir.path()));

        let no_marker = Activation {
            marker_files: vec![".nope".into()],
            ..Default::default()
        };
        assert!(!no_marker.is_active(dir.path()));
    }

    #[test]
    fn activation_env_var() {
        unsafe { std::env::set_var("TEST_PLUGIN_ACTIVE", "1"); }
        let activation = Activation {
            env_vars: vec!["TEST_PLUGIN_ACTIVE".into()],
            ..Default::default()
        };
        assert!(activation.is_active(std::path::Path::new(".")));
        unsafe { std::env::remove_var("TEST_PLUGIN_ACTIVE"); }
    }

    #[test]
    fn activation_always() {
        let activation = Activation { always: true, ..Default::default() };
        assert!(activation.is_active(std::path::Path::new("/nonexistent")));
    }

    #[test]
    fn resolve_template_with_env() {
        unsafe { std::env::set_var("TEST_SCRIBE_URL", "http://localhost:3000"); }
        let result = resolve_template("{TEST_SCRIBE_URL}/api/status", &HashMap::new());
        assert_eq!(result, "http://localhost:3000/api/status");
        unsafe { std::env::remove_var("TEST_SCRIBE_URL"); }
    }

    #[test]
    fn resolve_template_with_args() {
        let mut args = HashMap::new();
        args.insert("id".into(), "42".into());
        let result = resolve_template("http://localhost/api/engagement/{id}", &args);
        assert_eq!(result, "http://localhost/api/engagement/42");
    }
}
