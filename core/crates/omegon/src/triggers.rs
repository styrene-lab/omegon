//! Daemon trigger configuration — scheduled and event-driven prompt dispatch.
//!
//! Trigger configs live in `.omegon/triggers/*.toml`. Each config defines
//! either a **scheduled** trigger (runs on a timer) or an **event** trigger
//! (matches inbound `DaemonEventEnvelope` by source/trigger_kind and applies
//! a prompt template).
//!
//! # Scheduled triggers
//!
//! ```toml
//! [trigger]
//! name = "daily-review"
//! schedule = "daily"       # hourly | daily | weekdays | weekly
//! # OR: interval = "30m"  # 30s, 5m, 1h, 6h, etc.
//!
//! [prompt]
//! template = "Review open PRs and summarize status."
//!
//! [session]
//! caller_key = "trigger:daily-review"
//! ```
//!
//! # Event triggers (webhook template)
//!
//! ```toml
//! [trigger]
//! name = "github-pr"
//!
//! [filter]
//! source = "github"
//! trigger_kind = "prompt"
//!
//! [prompt]
//! template = "Review PR #{{payload.number}}: {{payload.title}}"
//! ```

use std::path::Path;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};

// ── Trigger config (deserialized from TOML) ──────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerConfig {
    pub trigger: TriggerMeta,
    pub filter: Option<TriggerFilter>,
    pub prompt: PromptTemplate,
    pub session: Option<SessionConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerMeta {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Preset schedule: hourly, daily, weekdays, weekly
    pub schedule: Option<String>,
    /// Interval duration: "30s", "5m", "1h", "6h"
    pub interval: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerFilter {
    pub source: Option<String>,
    pub trigger_kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptTemplate {
    pub template: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionConfig {
    pub caller_key: Option<String>,
}

// ── Schedule state ───────────────────────────────────────────────────────

/// Tracks when each scheduled trigger should next fire.
pub struct ScheduleState {
    entries: Vec<ScheduleEntry>,
}

struct ScheduleEntry {
    config: TriggerConfig,
    kind: ScheduleKind,
    last_fired: Option<Instant>,
    /// Wall-clock hour/minute of last daily/weekly fire (to avoid double-fire).
    last_fired_wall: Option<(u32, u32)>,
}

enum ScheduleKind {
    Interval(Duration),
    Preset(Preset),
}

#[derive(Debug, Clone, Copy)]
enum Preset {
    Hourly,
    Daily,
    Weekdays,
    Weekly,
}

impl ScheduleState {
    /// Build from loaded trigger configs, keeping only scheduled triggers.
    pub fn from_configs(configs: &[TriggerConfig]) -> Self {
        let entries = configs
            .iter()
            .filter(|c| c.trigger.enabled)
            .filter_map(|c| {
                let kind = if let Some(ref interval) = c.trigger.interval {
                    Some(ScheduleKind::Interval(parse_duration(interval)?))
                } else if let Some(ref schedule) = c.trigger.schedule {
                    Some(ScheduleKind::Preset(parse_preset(schedule)?))
                } else {
                    None
                };
                kind.map(|k| ScheduleEntry {
                    config: c.clone(),
                    kind: k,
                    last_fired: None,
                    last_fired_wall: None,
                })
            })
            .collect();
        Self { entries }
    }

    /// Check which triggers should fire now. Returns configs for those that
    /// are due. Call this from the dispatch loop's idle tick.
    pub fn poll_due(&mut self) -> Vec<TriggerConfig> {
        let now = Instant::now();
        let wall = Local::now();
        let mut due = Vec::new();

        for entry in &mut self.entries {
            let should_fire = match &entry.kind {
                ScheduleKind::Interval(d) => match entry.last_fired {
                    Some(last) => now.duration_since(last) >= *d,
                    None => true, // first tick fires immediately
                },
                ScheduleKind::Preset(preset) => {
                    preset_is_due(*preset, &wall, entry.last_fired_wall)
                }
            };

            if should_fire {
                entry.last_fired = Some(now);
                entry.last_fired_wall = Some((wall.hour(), wall.minute()));
                due.push(entry.config.clone());
            }
        }

        due
    }

    /// Number of scheduled triggers loaded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn preset_is_due(
    preset: Preset,
    wall: &chrono::DateTime<Local>,
    last_wall: Option<(u32, u32)>,
) -> bool {
    let (h, m) = (wall.hour(), wall.minute());

    // Avoid firing twice in the same minute.
    if last_wall == Some((h, m)) {
        return false;
    }

    match preset {
        Preset::Hourly => m == 0,
        Preset::Daily => h == 9 && m == 0,
        Preset::Weekdays => {
            let wd = wall.weekday().num_days_from_monday(); // 0=Mon, 6=Sun
            wd < 5 && h == 9 && m == 0
        }
        Preset::Weekly => {
            wall.weekday() == chrono::Weekday::Mon && h == 9 && m == 0
        }
    }
}

// ── Event matching ───────────────────────────────────────────────────────

/// All loaded event triggers (triggers with a `[filter]` section).
pub struct EventTriggers {
    triggers: Vec<TriggerConfig>,
}

impl EventTriggers {
    /// Build from loaded trigger configs, keeping only event (filter-based) triggers.
    pub fn from_configs(configs: &[TriggerConfig]) -> Self {
        let triggers = configs
            .iter()
            .filter(|c| c.trigger.enabled && c.filter.is_some())
            .cloned()
            .collect();
        Self { triggers }
    }

    /// Find the first trigger whose filter matches the given envelope fields.
    /// Returns the rendered prompt if a match is found.
    pub fn match_envelope(
        &self,
        source: &str,
        trigger_kind: &str,
        payload: &serde_json::Value,
    ) -> Option<MatchedTrigger> {
        for t in &self.triggers {
            let filter = t.filter.as_ref()?;
            if let Some(ref s) = filter.source {
                if s != source {
                    continue;
                }
            }
            if let Some(ref k) = filter.trigger_kind {
                if k != trigger_kind {
                    continue;
                }
            }
            // Filter matched — render the prompt template.
            let prompt = render_template(&t.prompt.template, payload);
            let caller_key = t
                .session
                .as_ref()
                .and_then(|s| s.caller_key.clone())
                .unwrap_or_else(|| format!("trigger:{}", t.trigger.name));
            return Some(MatchedTrigger {
                name: t.trigger.name.clone(),
                prompt,
                caller_key,
            });
        }
        None
    }

    pub fn len(&self) -> usize {
        self.triggers.len()
    }
}

/// Result of matching an inbound event against trigger configs.
pub struct MatchedTrigger {
    pub name: String,
    pub prompt: String,
    pub caller_key: String,
}

// ── Prompt template rendering ────────────────────────────────────────────

/// Render a template string, interpolating `{{payload.field}}` references
/// from the JSON payload. Nested access via dot notation (e.g.,
/// `{{payload.pull_request.title}}`). Missing fields render as empty string.
pub fn render_template(template: &str, payload: &serde_json::Value) -> String {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("}}") {
            let key = after_open[..end].trim();
            let value = resolve_template_key(key, payload);
            result.push_str(&value);
            rest = &after_open[end + 2..];
        } else {
            // Unclosed {{ — emit literally and stop.
            result.push_str(&rest[start..]);
            rest = "";
        }
    }
    result.push_str(rest);
    result
}

/// Resolve a dotted key like `payload.pull_request.title` against the
/// payload JSON value. The leading `payload.` prefix is stripped if present.
fn resolve_template_key(key: &str, payload: &serde_json::Value) -> String {
    let path = key.strip_prefix("payload.").unwrap_or(key);
    let mut current = payload;
    for segment in path.split('.') {
        match current.get(segment) {
            Some(v) => current = v,
            None => return String::new(),
        }
    }
    match current {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ── Config loading ───────────────────────────────────────────────────────

/// Load all trigger configs from `.omegon/triggers/`.
pub fn load_trigger_configs(cwd: &Path) -> Vec<TriggerConfig> {
    let dir = cwd.join(".omegon").join("triggers");
    if !dir.is_dir() {
        return Vec::new();
    }

    let mut configs = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return configs;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_config = path
            .extension()
            .is_some_and(|e| e == "toml" || e == "pkl");
        if !is_config {
            continue;
        }
        match load_single(&path) {
            Ok(config) => {
                tracing::info!(
                    name = %config.trigger.name,
                    schedule = ?config.trigger.schedule,
                    interval = ?config.trigger.interval,
                    has_filter = config.filter.is_some(),
                    format = ?path.extension().unwrap_or_default(),
                    "loaded trigger config"
                );
                configs.push(config);
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to parse trigger config"
                );
            }
        }
    }

    configs
}

fn load_single(path: &Path) -> anyhow::Result<TriggerConfig> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "pkl" => {
            let config: TriggerConfig = rpkl::from_config(path)
                .map_err(|e| anyhow::anyhow!("pkl: {e}"))?;
            Ok(config)
        }
        _ => {
            let content = std::fs::read_to_string(path)?;
            let config: TriggerConfig = toml::from_str(&content)?;
            Ok(config)
        }
    }
}

// ── Duration parsing ─────────────────────────────────────────────────────

/// Parse a human-friendly duration string: "30s", "5m", "1h", "6h", "1d".
fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let n: u64 = num.parse().ok()?;
    match unit {
        "s" => Some(Duration::from_secs(n)),
        "m" => Some(Duration::from_secs(n * 60)),
        "h" => Some(Duration::from_secs(n * 3600)),
        "d" => Some(Duration::from_secs(n * 86400)),
        _ => None,
    }
}

fn parse_preset(s: &str) -> Option<Preset> {
    match s.to_lowercase().as_str() {
        "hourly" => Some(Preset::Hourly),
        "daily" => Some(Preset::Daily),
        "weekdays" => Some(Preset::Weekdays),
        "weekly" => Some(Preset::Weekly),
        _ => None,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_duration_variants() {
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("2d"), Some(Duration::from_secs(172800)));
        assert!(parse_duration("abc").is_none());
        assert!(parse_duration("").is_none());
    }

    #[test]
    fn parse_preset_variants() {
        assert!(matches!(parse_preset("hourly"), Some(Preset::Hourly)));
        assert!(matches!(parse_preset("Daily"), Some(Preset::Daily)));
        assert!(matches!(parse_preset("WEEKDAYS"), Some(Preset::Weekdays)));
        assert!(matches!(parse_preset("weekly"), Some(Preset::Weekly)));
        assert!(parse_preset("biweekly").is_none());
    }

    #[test]
    fn render_template_simple() {
        let payload = json!({"text": "hello world"});
        assert_eq!(
            render_template("Say: {{payload.text}}", &payload),
            "Say: hello world"
        );
    }

    #[test]
    fn render_template_nested() {
        let payload = json!({"pr": {"number": 42, "title": "Fix bug"}});
        assert_eq!(
            render_template("PR #{{payload.pr.number}}: {{payload.pr.title}}", &payload),
            "PR #42: Fix bug"
        );
    }

    #[test]
    fn render_template_missing_key() {
        let payload = json!({"text": "hello"});
        assert_eq!(
            render_template("Value: {{payload.missing}}", &payload),
            "Value: "
        );
    }

    #[test]
    fn render_template_no_placeholders() {
        let payload = json!({});
        assert_eq!(
            render_template("Static prompt with no vars.", &payload),
            "Static prompt with no vars."
        );
    }

    #[test]
    fn render_template_unclosed_brace() {
        let payload = json!({});
        assert_eq!(
            render_template("Broken {{template", &payload),
            "Broken {{template"
        );
    }

    #[test]
    fn event_triggers_match() {
        let config = TriggerConfig {
            trigger: TriggerMeta {
                name: "gh-pr".into(),
                enabled: true,
                schedule: None,
                interval: None,
            },
            filter: Some(TriggerFilter {
                source: Some("github".into()),
                trigger_kind: Some("prompt".into()),
            }),
            prompt: PromptTemplate {
                template: "Review PR #{{payload.number}}".into(),
            },
            session: None,
        };

        let triggers = EventTriggers::from_configs(&[config]);
        let payload = json!({"number": 123, "text": "whatever"});

        let matched = triggers.match_envelope("github", "prompt", &payload);
        assert!(matched.is_some());
        let m = matched.unwrap();
        assert_eq!(m.prompt, "Review PR #123");
        assert_eq!(m.caller_key, "trigger:gh-pr");
    }

    #[test]
    fn event_triggers_no_match() {
        let config = TriggerConfig {
            trigger: TriggerMeta {
                name: "gh-pr".into(),
                enabled: true,
                schedule: None,
                interval: None,
            },
            filter: Some(TriggerFilter {
                source: Some("github".into()),
                trigger_kind: None,
            }),
            prompt: PromptTemplate {
                template: "Review".into(),
            },
            session: None,
        };

        let triggers = EventTriggers::from_configs(&[config]);
        let payload = json!({});

        // Wrong source
        assert!(triggers.match_envelope("slack", "prompt", &payload).is_none());
    }

    #[test]
    fn schedule_state_interval_fires() {
        let config = TriggerConfig {
            trigger: TriggerMeta {
                name: "fast".into(),
                enabled: true,
                schedule: None,
                interval: Some("1s".into()),
            },
            filter: None,
            prompt: PromptTemplate {
                template: "Do thing".into(),
            },
            session: None,
        };

        let mut state = ScheduleState::from_configs(&[config]);
        assert_eq!(state.len(), 1);

        // First poll should fire immediately.
        let due = state.poll_due();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].trigger.name, "fast");

        // Immediately again should NOT fire (interval not elapsed).
        let due = state.poll_due();
        assert!(due.is_empty());
    }

    #[test]
    fn toml_roundtrip() {
        let toml_str = r#"
[trigger]
name = "daily-review"
schedule = "daily"

[prompt]
template = "Review open PRs."

[session]
caller_key = "trigger:daily-review"
"#;
        let config: TriggerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.trigger.name, "daily-review");
        assert_eq!(config.trigger.schedule.as_deref(), Some("daily"));
        assert!(config.trigger.enabled); // default true
        assert!(config.filter.is_none());
        assert_eq!(config.prompt.template, "Review open PRs.");
        assert_eq!(
            config.session.unwrap().caller_key.as_deref(),
            Some("trigger:daily-review")
        );
    }

    #[test]
    fn disabled_triggers_excluded() {
        let config = TriggerConfig {
            trigger: TriggerMeta {
                name: "off".into(),
                enabled: false,
                schedule: Some("hourly".into()),
                interval: None,
            },
            filter: None,
            prompt: PromptTemplate {
                template: "nope".into(),
            },
            session: None,
        };

        let state = ScheduleState::from_configs(&[config.clone()]);
        assert_eq!(state.len(), 0);

        let triggers = EventTriggers::from_configs(&[config]);
        assert_eq!(triggers.len(), 0);
    }

    #[test]
    fn load_pkl_trigger_config() {
        let dir = tempfile::tempdir().unwrap();
        let pkl_path = dir.path().join("review.pkl");
        std::fs::write(
            &pkl_path,
            r#"
trigger {
  name = "pkl-review"
  schedule = "daily"
}

prompt {
  template = "Review open PRs via Pkl."
}
"#,
        )
        .unwrap();
        let config: TriggerConfig = load_single(&pkl_path).unwrap();
        assert_eq!(config.trigger.name, "pkl-review");
        assert_eq!(config.trigger.schedule.as_deref(), Some("daily"));
        assert!(config.trigger.enabled);
        assert_eq!(config.prompt.template, "Review open PRs via Pkl.");
    }

    #[test]
    fn load_pkl_trigger_with_filter() {
        let dir = tempfile::tempdir().unwrap();
        let pkl_path = dir.path().join("github.pkl");
        std::fs::write(
            &pkl_path,
            r#"
trigger {
  name = "gh-webhook"
  interval = "5m"
}

filter {
  source = "github"
  trigger_kind = "prompt"
}

prompt {
  template = "Handle GitHub event: {{payload.action}}"
}

session {
  caller_key = "trigger:gh-webhook"
}
"#,
        )
        .unwrap();
        let config: TriggerConfig = load_single(&pkl_path).unwrap();
        assert_eq!(config.trigger.name, "gh-webhook");
        assert_eq!(config.trigger.interval.as_deref(), Some("5m"));
        let filter = config.filter.unwrap();
        assert_eq!(filter.source.as_deref(), Some("github"));
        assert_eq!(filter.trigger_kind.as_deref(), Some("prompt"));
        assert_eq!(
            config.session.unwrap().caller_key.as_deref(),
            Some("trigger:gh-webhook")
        );
    }
}
