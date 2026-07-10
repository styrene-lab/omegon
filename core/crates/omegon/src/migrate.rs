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

use serde_json::{Value, json};
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
        Self {
            source: source.into(),
            items: vec![],
            warnings: vec![],
        }
    }

    fn add(&mut self, kind: &'static str, detail: impl Into<String>) {
        self.items.push(MigrationItem {
            kind,
            detail: detail.into(),
        });
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

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Detect which migration sources are present on this machine.
pub fn detect_sources() -> Vec<(&'static str, &'static str, bool)> {
    let h = home();
    vec![
        ("claude-code", "Claude Code", h.join(".claude").is_dir()),
        (
            "codex",
            "OpenAI Codex CLI",
            h.join(".codex").is_dir() || h.join(".config/codex").is_dir(),
        ),
        ("cursor", "Cursor IDE", cursor_settings_path().is_some()),
        ("aider", "Aider", h.join(".aider.conf.yml").exists()),
        (
            "continue",
            "Continue.dev",
            h.join(".continue/config.json").exists(),
        ),
        (
            "copilot",
            "GitHub Copilot",
            h.join(".config/github-copilot/hosts.json").exists(),
        ),
        (
            "windsurf",
            "Windsurf IDE",
            windsurf_settings_path().is_some(),
        ),
    ]
}

/// Run a migration by source name.
pub fn run(source: &str, cwd: &Path) -> MigrationReport {
    match source {
        "claude-code" | "claude" => migrate_claude_code(cwd),
        "codex" => migrate_codex(cwd),
        "cursor" => migrate_cursor(cwd),
        "aider" => migrate_aider(cwd),
        "continue" => migrate_continue(cwd),
        "copilot" => migrate_copilot(cwd),
        "windsurf" => migrate_windsurf(cwd),
        "auto" => migrate_auto(cwd),
        _ => {
            let mut r = MigrationReport::new(source);
            r.warn(format!("Unknown source: {source}. Try: auto, claude-code, codex, cursor, aider, continue, copilot, windsurf"));
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
    let priority = [
        "aider",
        "continue",
        "copilot",
        "cursor",
        "windsurf",
        "codex",
        "claude-code",
    ];
    for source in &priority {
        if available.iter().any(|(id, _, _)| id == source) {
            let sub = run(source, cwd);
            for item in sub.items {
                report.items.push(item);
            }
            for w in sub.warnings {
                report.warnings.push(w);
            }
        }
    }

    report
}

fn claude_skill_roots(cwd: &Path) -> Vec<(PathBuf, bool)> {
    let h = home();
    vec![
        (h.join(".claude/skills"), false),
        (h.join(".claude-code/skills"), false),
        (cwd.join(".claude/skills"), true),
        (cwd.join(".claude-code/skills"), true),
    ]
}

fn import_claude_skill(path: &Path, project: bool, cwd: &Path) -> anyhow::Result<()> {
    if project {
        let original = std::env::current_dir()?;
        struct Restore(std::path::PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        std::env::set_current_dir(cwd)?;
        let _restore = Restore(original);
        crate::skills::cmd_import(path, true, false)
    } else {
        crate::skills::cmd_import(path, false, false)
    }
}

fn migrate_claude_skills(cwd: &Path, r: &mut MigrationReport) {
    for (root, project) in claude_skill_roots(cwd) {
        if !root.is_dir() {
            continue;
        }
        let Ok(read_dir) = std::fs::read_dir(&root) else {
            r.warn(format!(
                "Failed to read Claude skills at {}",
                root.display()
            ));
            continue;
        };
        let mut bundles: Vec<_> = read_dir
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().join("SKILL.md").is_file())
            .collect();
        bundles.sort_by_key(|entry| entry.file_name());
        for bundle in bundles {
            let path = bundle.path();
            let display = path.display().to_string();
            match import_claude_skill(&path, project, cwd) {
                Ok(()) => r.add(
                    "skill",
                    format!(
                        "imported {} skill from {display}",
                        if project { "project" } else { "user" }
                    ),
                ),
                Err(err) => r.warn(format!(
                    "Skipped Claude skill at {display}: {err}. Re-run `omegon skills import {}{} --force` to refresh.",
                    shell_quote_path(&path),
                    if project { " --project" } else { "" }
                )),
            }
        }
    }
}

fn shell_quote_path(path: &Path) -> String {
    let s = path.display().to_string();
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':'))
    {
        s
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
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
            )
        {
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
            && !servers.is_empty()
        {
            write_mcp_config(servers, &mut r);
        }
    }

    // Settings from ~/.claude/settings.json
    if let Some(data) = read_json(&h.join(".claude/settings.json"))
        && let Some(model) = data.get("model").and_then(|v| v.as_str())
    {
        let full = expand_anthropic_model(model);
        r.add("model", &full);
        save_model_to_profile(cwd, &full);
    }

    // Project: CLAUDE.md → .omegon/AGENTS.md
    import_project_instructions(cwd, &cwd.join(".claude/CLAUDE.md"), &mut r);

    migrate_claude_skills(cwd, &mut r);

    r
}

// ─── pi / Omegon TS ─────────────────────────────────────────────────────────

fn migrate_pi(cwd: &Path) -> MigrationReport {
    let mut r = MigrationReport::new("pi / Omegon TS");
    let pi_dir = home().join(".pi/agent");

    // Auth — already in auth.json, we read it natively. Just report.
    if let Some(data) = read_json(&pi_dir.join("auth.json"))
        && let Some(obj) = data.as_object()
    {
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
        && let Some(servers) = data.get("servers").and_then(|v| v.as_object())
    {
        write_mcp_config(servers, &mut r);
    }

    // Project config
    if let Some(data) = read_json(&cwd.join(".pi/config.json"))
        && let Some(model) = data.get("lastUsedModel")
        && let (Some(p), Some(m)) = (
            model.get("provider").and_then(|v| v.as_str()),
            model.get("modelId").and_then(|v| v.as_str()),
        )
    {
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
            && let Some(model) = data.get("model").and_then(|v| v.as_str())
        {
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
        && let Some(data) = read_json(&settings_path)
    {
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
            && let Some(model) = data.get("model").and_then(|v| v.as_str())
        {
            // Aider uses bare model names like "claude-3-opus-20240229"
            let full = if model.contains('/') || model.contains(':') {
                model.to_string()
            } else if model.starts_with("claude") {
                format!("anthropic:{model}")
            } else if model.starts_with("gpt") || model.starts_with("o1") || model.starts_with("o3")
            {
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
            )
        {
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
        && let Some(obj) = data.as_object()
    {
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
        && settings_path.exists()
    {
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
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            map.insert(key, Value::String(value));
        }
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn expand_anthropic_model(short: &str) -> String {
    match short {
        "opus" | "opus4" | "opus4.6" => "anthropic:claude-opus-4-6",
        "sonnet" | "sonnet4" | "sonnet4.6" => "anthropic:claude-sonnet-4-6",
        "haiku" | "haiku4.5" => "anthropic:claude-haiku-4-5-20251001",
        other => {
            if other.contains(':') {
                return other.to_string();
            }
            if other.starts_with("claude") {
                return format!("anthropic:{other}");
            }
            other
        }
    }
    .to_string()
}

fn cursor_settings_path() -> Option<PathBuf> {
    let h = home();
    // macOS
    let mac = h.join("Library/Application Support/Cursor/User/settings.json");
    if mac.exists() {
        return Some(mac);
    }
    // Linux
    let linux = h.join(".config/Cursor/User/settings.json");
    if linux.exists() {
        return Some(linux);
    }
    None
}

fn windsurf_settings_path() -> Option<PathBuf> {
    let h = home();
    let mac = h.join("Library/Application Support/Windsurf/User/settings.json");
    if mac.exists() {
        return Some(mac);
    }
    let linux = h.join(".config/Windsurf/User/settings.json");
    if linux.exists() {
        return Some(linux);
    }
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
    if !source.exists() {
        return;
    }
    let target = cwd.join(".omegon/AGENTS.md");
    if target.exists() {
        r.warn(format!(
            ".omegon/AGENTS.md exists — skipped {}",
            source.file_name().unwrap_or_default().to_string_lossy()
        ));
        return;
    }
    let _ = std::fs::create_dir_all(target.parent().unwrap());
    if let Ok(content) = std::fs::read_to_string(source)
        && std::fs::write(&target, &content).is_ok()
    {
        r.add(
            "project-config",
            format!(
                "{} → .omegon/AGENTS.md",
                source.file_name().unwrap_or_default().to_string_lossy()
            ),
        );
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

// ═══════════════════════════════════════════════════════════════════════════
// /init — scan project for agent conventions and migrate to Omegon
// ═══════════════════════════════════════════════════════════════════════════

/// Detected agent convention in a project directory.
struct DetectedConvention {
    source: &'static str,
    description: String,
    path: PathBuf,
    kind: ConventionKind,
}

enum ConventionKind {
    /// Project instructions file → AGENTS.md
    Instructions,
    /// Memory/facts data → ai/memory/
    Memory,
    /// Agent config → .omegon/
    Config,
}

pub enum InitProfileScope {
    Project,
    User,
}

impl InitProfileScope {
    fn label(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
        }
    }
}

pub fn migrate_legacy_profile_to_registry(
    cwd: &Path,
    scope: InitProfileScope,
) -> anyhow::Result<String> {
    let (legacy_path, registry_dir, active_path, source) = match scope {
        InitProfileScope::Project => {
            let root = crate::setup::find_project_root(cwd);
            (
                root.join(".omegon/profile.json"),
                root.join(".omegon/profiles"),
                root.join(".omegon/active-profile.json"),
                "project",
            )
        }
        InitProfileScope::User => {
            let home =
                dirs::home_dir().ok_or_else(|| anyhow::anyhow!("home directory unavailable"))?;
            (
                home.join(".omegon/profile.json"),
                home.join(".omegon/profiles"),
                home.join(".omegon/active-profile.json"),
                "user",
            )
        }
    };
    if !legacy_path.exists() {
        anyhow::bail!("no legacy {source} profile at `{}`", legacy_path.display());
    }
    let content = std::fs::read_to_string(&legacy_path)?;
    let _: Profile = serde_json::from_str(&content)?;
    std::fs::create_dir_all(&registry_dir)?;
    let mut id = "default".to_string();
    let mut target = registry_dir.join("default.json");
    if target.exists() {
        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
        id = format!("legacy-{timestamp}");
        target = registry_dir.join(format!("{id}.json"));
    }
    std::fs::write(&target, content)?;
    if let Some(parent) = active_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let selection = crate::settings::ActiveProfileSelection {
        id: id.clone(),
        scope: Some(scope.label().to_string()),
    };
    std::fs::write(
        &active_path,
        serde_json::to_string_pretty(&selection)? + "\n",
    )?;
    Ok(format!(
        "✓ Migrated legacy {source} profile to `{}` and selected `{id}`. Legacy singleton was left in place for compatibility.",
        target.display()
    ))
}

/// Scan the project for agent conventions from other tools, migrate what's
/// found, and bootstrap the ai/ directory structure.
pub fn init_project(cwd: &Path, move_all: bool) -> String {
    let mut lines: Vec<String> = vec![];
    let detected = scan_conventions(cwd);
    let mut actions = 0;

    lines.push("## Project Scan\n".into());

    if detected.is_empty() {
        lines.push("No existing agent conventions detected.\n".into());
    } else {
        lines.push(format!("Found {} agent convention(s):\n", detected.len()));
        for d in &detected {
            lines.push(format!(
                "  • **{}** — {} (`{}`)",
                d.source,
                d.description,
                d.path.strip_prefix(cwd).unwrap_or(&d.path).display()
            ));
        }
        lines.push(String::new());
    }

    // ── Migrate instructions to AGENTS.md ────────────────────────────
    let agents_md = cwd.join("AGENTS.md");
    let omegon_agents = cwd.join(".omegon/AGENTS.md");
    if !agents_md.exists() && !omegon_agents.exists() {
        // Find the best instructions file to convert
        let instructions: Vec<&DetectedConvention> = detected
            .iter()
            .filter(|d| matches!(d.kind, ConventionKind::Instructions))
            .collect();
        if let Some(best) = instructions.first()
            && let Ok(content) = std::fs::read_to_string(&best.path)
        {
            let header = format!(
                "# Project Directives\n\n> Migrated from {} by Omegon /init\n\n",
                best.source
            );
            let _ = std::fs::write(&agents_md, format!("{header}{content}"));
            lines.push(format!("✓ Created `AGENTS.md` from {}", best.source));
            actions += 1;
        }
    } else if agents_md.exists() {
        lines.push("✓ `AGENTS.md` already exists".into());
    }

    // ── Migrate memory facts (.omegon/memory/ → ai/memory/) ─────────────
    let ai_memory = cwd.join("ai/memory");
    let omegon_memory = cwd.join(".omegon/memory");
    if !ai_memory.exists() && omegon_memory.join("facts.jsonl").exists() {
        let _ = std::fs::create_dir_all(&ai_memory);
        if let Ok(content) = std::fs::read_to_string(omegon_memory.join("facts.jsonl")) {
            let _ = std::fs::write(ai_memory.join("facts.jsonl"), &content);
            lines.push("✓ Migrated facts.jsonl from .omegon/memory → ai/memory/".into());
            actions += 1;
        }
        if omegon_memory.join("facts.db").exists() {
            let _ = std::fs::copy(omegon_memory.join("facts.db"), ai_memory.join("facts.db"));
            lines.push("✓ Migrated facts.db from .omegon/memory → ai/memory/".into());
            actions += 1;
        }
    }

    // ── Migrate design docs (docs/ → ai/docs/) ──────────────────────
    let ai_docs = cwd.join("ai/docs");
    let legacy_docs = cwd.join("docs");
    if !ai_docs.exists() && legacy_docs.is_dir() {
        // Check if docs/ actually has design tree markdown (frontmatter with status:)
        let has_design_docs = std::fs::read_dir(&legacy_docs)
            .ok()
            .map(|entries| {
                entries.filter_map(|e| e.ok()).any(|e| {
                    e.path().extension().is_some_and(|ext| ext == "md")
                        && std::fs::read_to_string(e.path())
                            .ok()
                            .is_some_and(|c| c.starts_with("---") && c.contains("status:"))
                })
            })
            .unwrap_or(false);

        if has_design_docs {
            if move_all {
                let _ = std::fs::create_dir_all(cwd.join("ai"));
                if std::fs::rename(&legacy_docs, &ai_docs).is_ok() {
                    lines.push("✓ Moved docs/ → ai/docs/".into());
                    actions += 1;
                } else {
                    lines.push("⚠ Failed to move docs/ → ai/docs/ (check permissions)".into());
                }
            } else {
                let count = std::fs::read_dir(&legacy_docs)
                    .map(|e| e.count())
                    .unwrap_or(0);
                lines.push(format!(
                    "📋 Design docs in `docs/` ({count} files) — Omegon reads from here.\n\
                     Run `/init migrate` to move to `ai/docs/`."
                ));
            }
        }
    }

    // ── Migrate OpenSpec (openspec/ → ai/openspec/) ──────────────────
    let ai_openspec = cwd.join("ai/openspec");
    let legacy_openspec = cwd.join("openspec");
    if !ai_openspec.exists() && legacy_openspec.is_dir() {
        if move_all {
            let _ = std::fs::create_dir_all(cwd.join("ai"));
            if std::fs::rename(&legacy_openspec, &ai_openspec).is_ok() {
                lines.push("✓ Moved openspec/ → ai/openspec/".into());
                actions += 1;
            } else {
                lines.push("⚠ Failed to move openspec/ → ai/openspec/ (check permissions)".into());
            }
        } else {
            lines.push(
                "📋 OpenSpec in `openspec/` — Omegon reads from here.\n\
                 Run `/init migrate` to move to `ai/openspec/`."
                    .into(),
            );
        }
    }

    // ── Migrate lifecycle state (.omegon/lifecycle/ → ai/lifecycle/) ─
    let ai_lifecycle = cwd.join("ai/lifecycle");
    let omegon_lifecycle = cwd.join(".omegon/lifecycle");
    if !ai_lifecycle.exists() && omegon_lifecycle.join("state.json").exists() {
        let _ = std::fs::create_dir_all(&ai_lifecycle);
        let _ = std::fs::copy(
            omegon_lifecycle.join("state.json"),
            ai_lifecycle.join("state.json"),
        );
        lines.push("✓ Migrated lifecycle state → ai/lifecycle/".into());
        actions += 1;
    }

    // ── Migrate milestones (.omegon/milestones.json → ai/) ──────────
    let ai_milestones = cwd.join("ai/milestones.json");
    let omegon_milestones = cwd.join(".omegon/milestones.json");
    if !ai_milestones.exists() && omegon_milestones.exists() {
        let _ = std::fs::create_dir_all(cwd.join("ai"));
        let _ = std::fs::copy(&omegon_milestones, &ai_milestones);
        lines.push("✓ Migrated milestones.json → ai/".into());
        actions += 1;
    }

    // ── Bootstrap canonical project memory if explicitly initialized ───
    let ai_memory = cwd.join("ai/memory");
    if !ai_memory.exists() {
        if std::fs::create_dir_all(&ai_memory).is_ok() {
            lines.push("✓ Created `ai/memory/` for durable project facts".into());
            actions += 1;
        } else {
            lines.push("⚠ Failed to create `ai/memory/` (check permissions)".into());
        }
    } else {
        lines.push("✓ `ai/memory/` already exists".into());
    }

    // ── Bootstrap .omegon/ config dir if needed ──────────────────────
    let config_dir = cwd.join(".omegon");
    if !config_dir.exists() {
        let _ = std::fs::create_dir_all(&config_dir);
        lines.push("✓ Created `.omegon/` config directory".into());
        actions += 1;
    }

    // ── User-level auth migration (runs once, regardless of project) ─
    let user_migration = migrate_user_config();
    if !user_migration.is_empty() {
        lines.push(String::new());
        lines.push("### User Config".into());
        for msg in user_migration {
            lines.push(msg);
            actions += 1;
        }
    }

    // ── Summary ──────────────────────────────────────────────────────
    lines.push(String::new());
    if actions > 0 {
        lines.push(format!("**{actions} action(s) completed.**"));
        if move_all {
            lines.push("\n⚠ Restart Omegon to pick up the new ai/ paths.".into());
        }
    } else {
        lines.push("Project is already configured for Omegon.".into());
    }

    // Show final directory layout
    lines.push(String::new());
    lines.push("### Directory Layout".into());
    let ai_exists = cwd.join("ai").is_dir();
    if ai_exists {
        lines.push("```".into());
        lines.push("ai/".into());
        for sub in [
            "docs/",
            "openspec/",
            "memory/",
            "lifecycle/",
            "milestones.json",
        ] {
            let path = cwd.join("ai").join(sub);
            let marker = if path.exists() { "✓" } else { " " };
            lines.push(format!("  {marker} {sub}"));
        }
        lines.push(".omegon/          (tool config)".into());
        lines.push("AGENTS.md         (project directives)".into());
        lines.push("```".into());
    } else {
        lines.push(
            "No `ai/` directory yet. Legacy paths (`docs/`, `openspec/`) are supported.".into(),
        );
        lines.push(
            "To adopt the `ai/` convention, move your design docs and OpenSpec there.".into(),
        );
    }

    lines.join("\n")
}

/// Migrate user-level config from legacy locations to ~/.config/omegon/.
/// Returns a list of actions taken (empty if nothing to do).
fn migrate_user_config() -> Vec<String> {
    Vec::new()
}

/// Scan for agent conventions in the project directory.
fn scan_conventions(cwd: &Path) -> Vec<DetectedConvention> {
    let mut found = Vec::new();

    // Claude Code: CLAUDE.md or .claude/CLAUDE.md
    for path in [cwd.join("CLAUDE.md"), cwd.join(".claude/CLAUDE.md")] {
        if path.exists() {
            found.push(DetectedConvention {
                source: "Claude Code",
                description: "Project instructions".into(),
                path,
                kind: ConventionKind::Instructions,
            });
            break;
        }
    }

    // Codex: codex.md
    if cwd.join("codex.md").exists() {
        found.push(DetectedConvention {
            source: "OpenAI Codex",
            description: "Project instructions".into(),
            path: cwd.join("codex.md"),
            kind: ConventionKind::Instructions,
        });
    }

    // Cursor: .cursor/rules or .cursorrules
    for path in [cwd.join(".cursor/rules"), cwd.join(".cursorrules")] {
        if path.exists() {
            found.push(DetectedConvention {
                source: "Cursor",
                description: "Project rules".into(),
                path,
                kind: ConventionKind::Instructions,
            });
            break;
        }
    }

    // Windsurf: .windsurfrules
    if cwd.join(".windsurfrules").exists() {
        found.push(DetectedConvention {
            source: "Windsurf",
            description: "Project rules".into(),
            path: cwd.join(".windsurfrules"),
            kind: ConventionKind::Instructions,
        });
    }

    // Aider: .aider.conf.yml
    if cwd.join(".aider.conf.yml").exists() {
        found.push(DetectedConvention {
            source: "Aider",
            description: "Configuration".into(),
            path: cwd.join(".aider.conf.yml"),
            kind: ConventionKind::Config,
        });
    }

    // Cline: .clinerules
    if cwd.join(".clinerules").exists() {
        found.push(DetectedConvention {
            source: "Cline",
            description: "Project rules".into(),
            path: cwd.join(".clinerules"),
            kind: ConventionKind::Instructions,
        });
    }

    // GitHub Copilot: .github/copilot-instructions.md
    if cwd.join(".github/copilot-instructions.md").exists() {
        found.push(DetectedConvention {
            source: "GitHub Copilot",
            description: "Copilot instructions".into(),
            path: cwd.join(".github/copilot-instructions.md"),
            kind: ConventionKind::Instructions,
        });
    }

    // pi / Omegon TS: .pi/memory/
    if cwd.join(".pi/memory/facts.jsonl").exists() {
        found.push(DetectedConvention {
            source: "pi (Omegon TS)",
            description: "Memory facts".into(),
            path: cwd.join(".pi/memory"),
            kind: ConventionKind::Memory,
        });
    }

    // Legacy .omegon/memory/ (pre-ai/ convention)
    if cwd.join(".omegon/memory/facts.jsonl").exists() && !cwd.join("ai/memory").exists() {
        found.push(DetectedConvention {
            source: "Omegon (legacy)",
            description: "Memory facts in .omegon/".into(),
            path: cwd.join(".omegon/memory"),
            kind: ConventionKind::Memory,
        });
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvRestore {
        key: &'static str,
        value: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                value: previous,
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.value {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

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
        assert!(
            !report.warnings.is_empty() || report.items.is_empty(),
            "unknown source should warn or have no items"
        );
    }

    #[test]
    fn migrate_claude_code_imports_project_skill_bundle() {
        let _guard = crate::test_support::env::lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvRestore::set("OMEGON_HOME", home.path());
        let skill_dir = cwd.path().join(".claude/skills/project-helper");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: project-helper\ndescription: Project helper\n---\n\nBody\n",
        )
        .unwrap();
        std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        std::fs::write(skill_dir.join("scripts/run.sh"), "echo ok\n").unwrap();

        let report = migrate_claude_code(cwd.path());

        assert!(report.items.iter().any(|item| item.kind == "skill"));
        assert!(
            cwd.path()
                .join(".omegon/skills/project-helper/SKILL.md")
                .is_file()
        );
        assert!(
            cwd.path()
                .join(".omegon/skills/project-helper/scripts/run.sh")
                .is_file()
        );
    }

    #[test]
    fn migrate_claude_code_skips_existing_skill_with_force_hint() {
        let _guard = crate::test_support::env::lock();
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvRestore::set("OMEGON_HOME", home.path());
        let skill_dir = cwd.path().join(".claude/skills/existing-helper");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: existing-helper\ndescription: Existing helper\n---\n\nBody\n",
        )
        .unwrap();
        let destination = cwd.path().join(".omegon/skills/existing-helper");
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(destination.join("SKILL.md"), "already here\n").unwrap();

        let report = migrate_claude_code(cwd.path());

        assert!(report.warnings.iter().any(|warning| {
            warning.contains("existing-helper")
                && warning.contains("omegon skills import")
                && warning.contains("--project")
                && warning.contains("--force")
        }));
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
        assert!(
            summary.contains("malformed") || summary.contains("warning"),
            "should surface warnings: {summary}"
        );
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
        std::fs::write(
            dir.path().join(".windsurfrules"),
            "Always use TypeScript\nPrefer functional style\n",
        )
        .unwrap();
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

    // ── /init tests ─────────────────────────────────────────────────────

    #[test]
    fn scan_detects_claude_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "# My Rules\nBe nice").unwrap();
        let found = scan_conventions(dir.path());
        assert!(
            found.iter().any(|d| d.source == "Claude Code"),
            "should detect CLAUDE.md"
        );
    }

    #[test]
    fn scan_detects_copilot_instructions() {
        let dir = tempfile::tempdir().unwrap();
        let gh = dir.path().join(".github");
        std::fs::create_dir_all(&gh).unwrap();
        std::fs::write(
            gh.join("copilot-instructions.md"),
            "# Copilot\nUse TypeScript",
        )
        .unwrap();
        let found = scan_conventions(dir.path());
        assert!(
            found.iter().any(|d| d.source == "GitHub Copilot"),
            "should detect copilot-instructions.md"
        );
    }

    #[test]
    fn scan_detects_cline_rules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".clinerules"), "use strict mode").unwrap();
        let found = scan_conventions(dir.path());
        assert!(
            found.iter().any(|d| d.source == "Cline"),
            "should detect .clinerules"
        );
    }

    #[test]
    fn init_empty_project_creates_config() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_project(dir.path(), false);
        assert!(
            report.contains("Created `.omegon/` config directory"),
            "{report}"
        );
        assert!(dir.path().join(".omegon").is_dir());
    }

    #[test]
    fn init_project_creates_memory_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_project(dir.path(), false);
        assert!(
            report.contains("Created `ai/memory/` for durable project facts"),
            "{report}"
        );
        assert!(dir.path().join("ai/memory").is_dir());
    }

    #[test]
    fn init_migrates_claude_md_to_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "# Rules\nBe concise").unwrap();
        let report = init_project(dir.path(), false);
        assert!(report.contains("Created `AGENTS.md`"), "{report}");
        let content = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(content.contains("Be concise"));
        assert!(content.contains("Migrated from Claude Code"));
    }

    #[test]
    fn init_existing_project_reports_already_configured() {
        let dir = tempfile::tempdir().unwrap();
        // Pre-create the config dir and AGENTS.md
        std::fs::create_dir_all(dir.path().join(".omegon")).unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "# My Directives").unwrap();
        let report = init_project(dir.path(), false);
        // Should mention AGENTS.md exists, not try to create it
        assert!(report.contains("already exists"), "{report}");
    }
}
