//! Migration — import settings and auth from other CLI agent tools.
//!
//! Supported sources:
//!   claude-code  — Claude Code (~/.claude/, ~/.claude.json)
//!   pi           — pi / Omegon TS (~/.pi/agent/)
//!   codex        — OpenAI Codex CLI (~/.codex/, ~/.config/codex/)
//!   cursor       — Cursor IDE (.cursor/rules, VS Code settings)
//!   aider        — Aider (.aider.conf.yml)
//!   continue     — Continue.dev (~/.continue/config.json)
//!   copilot      — GitHub Copilot (~/.config/github-copilot/)
//!   windsurf     — Windsurf IDE (.windsurfrules)
//!
//! Each migrator:
//!   1. Probes for the tool's config files
//!   2. Extracts auth, model preferences, MCP servers, project instructions
//!   3. Writes to .omegon/profile.json and ~/.config/omegon/

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::settings::{Profile, ProfileModel};

/// What was found and imported.
pub struct MigrationReport {
    pub source: String,
    pub items: Vec<MigrationItem>,
    pub warnings: Vec<String>,
}

pub struct MigrationItem {
    pub kind: &'static str, // "auth", "model", "thinking", "mcp", "project-config"
    pub detail: String,
}

impl MigrationReport {
    fn new(source: &str) -> Self {
        Self { source: source.into(), items: vec![], warnings: vec![] }
    }

    fn add(&mut self, kind: &'static str, detail: impl Into<String>) {
        self.items.push(MigrationItem { kind, detail: detail.into() });
    }

    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![format!("Migration from {}:", self.source)];
        if self.items.is_empty() && self.warnings.is_empty() {
            lines.push("  (nothing found to import)".into());
        }
        for item in &self.items {
            lines.push(format!("  ✓ {}: {}", item.kind, item.detail));
        }
        for w in &self.warnings {
            lines.push(format!("  ⚠ {w}"));
        }
        lines.join("\n")
    }
}

fn home() -> PathBuf { dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")) }

/// Detect which migration sources are present on this machine.
pub fn detect_sources() -> Vec<(&'static str, &'static str, bool)> {
    let h = home();
    vec![
        ("claude-code", "Claude Code",     h.join(".claude").is_dir()),
        ("pi",          "pi / Omegon TS",  h.join(".pi/agent").is_dir()),
        ("codex",       "OpenAI Codex CLI", h.join(".codex").is_dir() || h.join(".config/codex").is_dir()),
        ("cursor",      "Cursor IDE",      cursor_settings_path().is_some()),
        ("aider",       "Aider",           h.join(".aider.conf.yml").exists()),
        ("continue",    "Continue.dev",    h.join(".continue/config.json").exists()),
        ("copilot",     "GitHub Copilot",  h.join(".config/github-copilot/hosts.json").exists()),
        ("windsurf",    "Windsurf IDE",    windsurf_settings_path().is_some()),
    ]
}

/// Run a migration by source name.
pub fn run(source: &str, cwd: &Path) -> MigrationReport {
    match source {
        "claude-code" | "claude" => migrate_claude_code(cwd),
        "pi" | "omegon"         => migrate_pi(cwd),
        "codex"                 => migrate_codex(cwd),
        "cursor"                => migrate_cursor(cwd),
        "aider"                 => migrate_aider(cwd),
        "continue"              => migrate_continue(cwd),
        "copilot"               => migrate_copilot(cwd),
        "windsurf"              => migrate_windsurf(cwd),
        "auto"                  => migrate_auto(cwd),
        _ => {
            let mut r = MigrationReport::new(source);
            r.warn(format!("Unknown source: {source}. Try: auto, claude-code, pi, codex, cursor, aider, continue, copilot, windsurf"));
            r
        }
    }
}

/// Auto-detect and migrate from whatever is available.
fn migrate_auto(cwd: &Path) -> MigrationReport {
    let mut report = MigrationReport::new("auto-detect");
    let sources = detect_sources();
    let available: Vec<_> = sources.iter().filter(|(_, _, found)| *found).collect();

    if available.is_empty() {
        report.warn("No existing CLI agent tools detected");
        return report;
    }

    for (id, name, _) in &available {
        report.add("detected", format!("{name} ({id})"));
    }

    // Migrate in priority order — later sources override earlier ones
    let priority = ["aider", "continue", "copilot", "cursor", "windsurf", "codex", "pi", "claude-code"];
    for source in &priority {
        if available.iter().any(|(id, _, _)| id == source) {
            let sub = run(source, cwd);
            for item in sub.items { report.items.push(item); }
            for w in sub.warnings { report.warnings.push(w); }
        }
    }

    report
}

// ─── Claude Code ────────────────────────────────────────────────────────────

fn migrate_claude_code(cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("Claude Code");
    let h = home();

    // Auth from ~/.claude.json
    let claude_json = h.join(".claude.json");
    if let Some(data) = read_json(&claude_json) {
        if let Some(oauth) = data.get("oauthAccount")
            && let (Some(access), Some(refresh), Some(expires)) = (
                oauth.get("accessToken").and_then(|v| v.as_str()),
                oauth.get("refreshToken").and_then(|v| v.as_str()),
                oauth.get("expiresAt").and_then(|v| v.as_i64()),
            ) {
                let creds = crate::auth::OAuthCredentials {
                    cred_type: "oauth".into(),
                    access: access.into(),
                    refresh: refresh.into(),
                    expires: expires as u64,
                };
                match crate::auth::write_credentials("anthropic", &creds) {
                    Ok(_) => r.add("auth", "Anthropic OAuth from Claude Code"),
                    Err(e) => r.warn(format!("Failed to import auth: {e}")),
                }
            }

        // MCP servers
        if let Some(servers) = data.get("mcpServers").and_then(|v| v.as_object())
            && !servers.is_empty() {
                write_mcp_config(servers, &mut r);
            }
    }

    // Settings from ~/.claude/settings.json
    if let Some(data) = read_json(&h.join(".claude/settings.json"))
        && let Some(model) = data.get("model").and_then(|v| v.as_str()) {
            let full = expand_anthropic_model(model);
            r.add("model", &full);
            save_model_to_profile(cwd, &full);
        }

    // Project: CLAUDE.md → .omegon/AGENTS.md
    import_project_instructions(cwd, &cwd.join(".claude/CLAUDE.md"), &mut r);

    r
}

// ─── pi / Omegon TS ─────────────────────────────────────────────────────────

fn migrate_pi(cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("pi / Omegon TS");
    let pi_dir = home().join(".pi/agent");

    // Auth — already in auth.json, we read it natively. Just report.
    if let Some(data) = read_json(&pi_dir.join("auth.json"))
        && let Some(obj) = data.as_object() {
            for key in obj.keys() {
                r.add("auth", format!("{key} (already in auth.json)"));
            }
        }

    // Settings
    if let Some(data) = read_json(&pi_dir.join("settings.json")) {
        if let (Some(provider), Some(model)) = (
            data.get("defaultProvider").and_then(|v| v.as_str()),
            data.get("defaultModel").and_then(|v| v.as_str()),
        ) {
            let full = format!("{provider}:{model}");
            r.add("model", &full);
            save_model_to_profile(cwd, &full);
        }
        if let Some(thinking) = data.get("defaultThinkingLevel").and_then(|v| v.as_str()) {
            r.add("thinking", thinking);
            save_thinking_to_profile(cwd, thinking);
        }
    }

    // MCP servers
    if let Some(data) = read_json(&pi_dir.join("mcp.json"))
        && let Some(servers) = data.get("servers").and_then(|v| v.as_object()) {
            write_mcp_config(servers, &mut r);
        }

    // Project config
    if let Some(data) = read_json(&cwd.join(".pi/config.json"))
        && let Some(model) = data.get("lastUsedModel")
            && let (Some(p), Some(m)) = (
                model.get("provider").and_then(|v| v.as_str()),
                model.get("modelId").and_then(|v| v.as_str()),
            ) {
                r.add("project-model", format!("{p}:{m}"));
                save_model_to_profile(cwd, &format!("{p}:{m}"));
            }

    r
}

// ─── OpenAI Codex CLI ───────────────────────────────────────────────────────

fn migrate_codex(cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("OpenAI Codex CLI");
    let h = home();

    // Try ~/.codex/ and ~/.config/codex/
    for dir in [h.join(".codex"), h.join(".config/codex")] {
        if let Some(data) = read_json(&dir.join("config.json"))
            .or_else(|| read_yaml_as_json(&dir.join("config.yaml")))
            && let Some(model) = data.get("model").and_then(|v| v.as_str()) {
                let full = format!("openai:{model}");
                r.add("model", &full);
                save_model_to_profile(cwd, &full);
            }
    }

    // OpenAI auth from env
    if std::env::var("OPENAI_API_KEY").is_ok() {
        r.add("auth", "OPENAI_API_KEY from environment");
    }

    // Project: codex.md → .omegon/AGENTS.md
    import_project_instructions(cwd, &cwd.join("codex.md"), &mut r);
    import_project_instructions(cwd, &cwd.join("AGENTS.md"), &mut r);

    r
}

// ─── Cursor ─────────────────────────────────────────────────────────────────

fn migrate_cursor(cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("Cursor IDE");

    if let Some(settings_path) = cursor_settings_path()
        && let Some(data) = read_json(&settings_path) {
            // Cursor stores AI model in various keys
            for key in ["cursor.aiModel", "cursor.model", "ai.model"] {
                if let Some(model) = data.get(key).and_then(|v| v.as_str()) {
                    r.add("model", model);
                    break;
                }
            }
        }

    // Project: .cursor/rules or .cursorrules → .omegon/AGENTS.md
    import_project_instructions(cwd, &cwd.join(".cursor/rules"), &mut r);
    import_project_instructions(cwd, &cwd.join(".cursorrules"), &mut r);

    r
}

// ─── Aider ──────────────────────────────────────────────────────────────────

fn migrate_aider(cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("Aider");

    // Global config
    for path in [home().join(".aider.conf.yml"), cwd.join(".aider.conf.yml")] {
        if let Some(data) = read_yaml_as_json(&path)
            && let Some(model) = data.get("model").and_then(|v| v.as_str()) {
                // Aider uses bare model names like "claude-3-opus-20240229"
                let full = if model.contains('/') || model.contains(':') {
                    model.to_string()
                } else if model.starts_with("claude") {
                    format!("anthropic:{model}")
                } else if model.starts_with("gpt") || model.starts_with("o1") || model.starts_with("o3") {
                    format!("openai:{model}")
                } else {
                    model.to_string()
                };
                r.add("model", &full);
                save_model_to_profile(cwd, &full);
            }
    }

    // Aider uses env vars for auth
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        r.add("auth", "ANTHROPIC_API_KEY from environment");
    }
    if std::env::var("OPENAI_API_KEY").is_ok() {
        r.add("auth", "OPENAI_API_KEY from environment");
    }

    r
}

// ─── Continue.dev ───────────────────────────────────────────────────────────

fn migrate_continue(cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("Continue.dev");

    if let Some(data) = read_json(&home().join(".continue/config.json")) {
        // Continue stores models in a "models" array
        if let Some(models) = data.get("models").and_then(|v| v.as_array())
            && let Some(first) = models.first()
                && let (Some(provider), Some(model)) = (
                    first.get("provider").and_then(|v| v.as_str()),
                    first.get("model").and_then(|v| v.as_str()),
                ) {
                    r.add("model", format!("{provider}:{model}"));
                }
    }

    // Project: .continuerc.json
    if cwd.join(".continuerc.json").exists() {
        r.add("project-config", ".continuerc.json found");
    }

    r
}

// ─── GitHub Copilot ─────────────────────────────────────────────────────────

fn migrate_copilot(_cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("GitHub Copilot");

    let hosts = home().join(".config/github-copilot/hosts.json");
    if let Some(data) = read_json(&hosts)
        && let Some(obj) = data.as_object() {
            for (host, _) in obj {
                r.add("auth", format!("GitHub OAuth ({host})"));
            }
        }

    r
}

// ─── Windsurf ───────────────────────────────────────────────────────────────

fn migrate_windsurf(cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("Windsurf IDE");

    if let Some(settings_path) = windsurf_settings_path()
        && settings_path.exists() {
            r.add("detected", settings_path.display().to_string());
        }

    // Project: .windsurfrules → .omegon/AGENTS.md
    import_project_instructions(cwd, &cwd.join(".windsurfrules"), &mut r);

    r
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn read_json(path: &Path) -> Option<Value> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn read_yaml_as_json(path: &Path) -> Option<Value> {
    // Simple YAML key: value parsing (no full YAML parser — handles flat configs)
    let content = std::fs::read_to_string(path).ok()?;
    let mut map = serde_json::Map::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
            map.insert(key, Value::String(value));
        }
    }
    if map.is_empty() { None } else { Some(Value::Object(map)) }
}

fn expand_anthropic_model(short: &str) -> String {
    match short {
        "opus" | "opus4" | "opus4.6" => "anthropic:claude-opus-4-6",
        "sonnet" | "sonnet4" | "sonnet4.6" => "anthropic:claude-sonnet-4-6",
        "haiku" | "haiku4.5" => "anthropic:claude-haiku-4-5-20251001",
        other => {
            if other.contains(':') { return other.to_string(); }
            if other.starts_with("claude") { return format!("anthropic:{other}"); }
            other
        }
    }.to_string()
}

fn cursor_settings_path() -> Option<PathBuf> {
    let h = home();
    // macOS
    let mac = h.join("Library/Application Support/Cursor/User/settings.json");
    if mac.exists() { return Some(mac); }
    // Linux
    let linux = h.join(".config/Cursor/User/settings.json");
    if linux.exists() { return Some(linux); }
    None
}

fn windsurf_settings_path() -> Option<PathBuf> {
    let h = home();
    let mac = h.join("Library/Application Support/Windsurf/User/settings.json");
    if mac.exists() { return Some(mac); }
    let linux = h.join(".config/Windsurf/User/settings.json");
    if linux.exists() { return Some(linux); }
    None
}

fn write_mcp_config(servers: &serde_json::Map<String, Value>, r: &mut MigrationReport) {
    let config = json!({ "servers": servers });
    let target = home().join(".config/omegon/mcp.json");
    let _ = std::fs::create_dir_all(target.parent().unwrap());
    if let Ok(json) = serde_json::to_string_pretty(&config) {
        let _ = std::fs::write(&target, json);
        for name in servers.keys() {
            r.add("mcp", name.clone());
        }
    }
}

fn import_project_instructions(cwd: &Path, source: &Path, r: &mut MigrationReport) {
    if !source.exists() { return; }
    let target = cwd.join(".omegon/AGENTS.md");
    if target.exists() {
        r.warn(format!(".omegon/AGENTS.md exists — skipped {}", source.file_name().unwrap_or_default().to_string_lossy()));
        return;
    }
    let _ = std::fs::create_dir_all(target.parent().unwrap());
    if let Ok(content) = std::fs::read_to_string(source)
        && std::fs::write(&target, &content).is_ok() {
            r.add("project-config", format!("{} → .omegon/AGENTS.md", source.file_name().unwrap_or_default().to_string_lossy()));
        }
}

fn save_model_to_profile(cwd: &Path, model: &str) {
    let mut profile = Profile::load(cwd);
    let parts: Vec<&str> = model.splitn(2, ':').collect();
    if parts.len() == 2 {
        profile.last_used_model = Some(ProfileModel {
            provider: parts[0].to_string(),
            model_id: parts[1].to_string(),
        });
    }
    let _ = profile.save(cwd);
}

fn save_thinking_to_profile(cwd: &Path, level: &str) {
    let mut profile = Profile::load(cwd);
    profile.thinking_level = Some(level.to_string());
    let _ = profile.save(cwd);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_sources_returns_list() {
        let sources = detect_sources();
        assert!(!sources.is_empty(), "should list at least some sources");
        // Every source should have a name and description
        for (name, desc, _found) in &sources {
            assert!(!name.is_empty());
            assert!(!desc.is_empty());
        }
    }

    #[test]
    fn run_auto_doesnt_panic() {
        let dir = tempfile::tempdir().unwrap();
        let report = run("auto", dir.path());
        assert_eq!(report.source, "auto-detect");
        // Should complete without panic even with no sources
    }

    #[test]
    fn run_unknown_source() {
        let dir = tempfile::tempdir().unwrap();
        let report = run("nonexistent", dir.path());
        assert!(!report.warnings.is_empty() || report.items.is_empty(),
            "unknown source should warn or have no items");
    }

    #[test]
    fn migration_report_summary() {
        let mut report = MigrationReport::new("test");
        assert!(report.summary().contains("test"));

        report.add("auth", "Found API key");
        report.add("model", "claude-sonnet-4");
        assert!(report.summary().contains("auth"));
        assert!(report.summary().contains("model"));
    }

    #[test]
    fn migration_report_with_warnings() {
        let mut report = MigrationReport::new("test");
        report.warnings.push("Config file malformed".into());
        let summary = report.summary();
        assert!(summary.contains("malformed") || summary.contains("warning"),
            "should surface warnings: {summary}");
    }

    #[test]
    fn migrate_cursor_from_empty() {
        let dir = tempfile::tempdir().unwrap();
        let report = migrate_cursor(dir.path());
        // Should complete without panic
        assert_eq!(report.source, "Cursor IDE");
    }

    #[test]
    fn migrate_aider_from_empty() {
        let dir = tempfile::tempdir().unwrap();
        let report = migrate_aider(dir.path());
        assert_eq!(report.source, "Aider");
    }

    #[test]
    fn migrate_windsurf_from_empty() {
        let dir = tempfile::tempdir().unwrap();
        let report = migrate_windsurf(dir.path());
        assert_eq!(report.source, "Windsurf IDE");
    }

    #[test]
    fn migrate_windsurf_with_rules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".windsurfrules"), "Always use TypeScript\nPrefer functional style\n").unwrap();
        let report = migrate_windsurf(dir.path());
        assert!(!report.items.is_empty(), "should find windsurf rules");
    }

    #[test]
    fn migrate_cursor_with_rules() {
        let dir = tempfile::tempdir().unwrap();
        let cursor_dir = dir.path().join(".cursor");
        std::fs::create_dir_all(&cursor_dir).unwrap();
        std::fs::write(cursor_dir.join("rules"), "Use Rust\nNo unwrap\n").unwrap();
        let report = migrate_cursor(dir.path());
        assert!(!report.items.is_empty(), "should find cursor rules");
    }
}
