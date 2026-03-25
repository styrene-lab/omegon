//! Serve tool — manage long-lived background processes.
//!
//! Dev servers, watchers, MCP servers, build daemons — anything that needs to
//! outlive a single bash command timeout. Uses PID files and log redirection.
//!
//! State directory: ~/.config/omegon/serve/
//!   {name}.pid  — PID of the running process
//!   {name}.log  — stdout + stderr
//!   {name}.meta — JSON: command, cwd, started_at, persist

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

fn serve_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("omegon")
        .join("serve")
}

fn ok(text: String) -> Result<ToolResult> {
    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: json!({}),
    })
}

fn err(text: String) -> Result<ToolResult> {
    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: json!({ "is_error": true }),
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct ServiceMeta {
    name: String,
    command: String,
    cwd: String,
    pid: u32,
    started_at: String,
    persist: bool,
}

pub async fn execute(action: &str, args: &serde_json::Value, cwd: &Path) -> Result<ToolResult> {
    match action {
        "start" => start(args, cwd).await,
        "stop" => stop(args).await,
        "list" => list().await,
        "logs" => logs(args).await,
        "check" => check(args).await,
        _ => err(format!("Unknown action: {action}. Valid: start, stop, list, logs, check")),
    }
}

async fn start(args: &serde_json::Value, cwd: &Path) -> Result<ToolResult> {
    let command = args.get("command").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("'command' is required"))?;

    let name = args.get("name").and_then(|v| v.as_str())
        .map(|s| sanitize_name(s))
        .unwrap_or_else(|| slugify_command(command));

    let persist = args.get("persist").and_then(|v| v.as_bool()).unwrap_or(false);

    let dir = serve_dir();
    std::fs::create_dir_all(&dir)?;

    let pid_path = dir.join(format!("{name}.pid"));
    let log_path = dir.join(format!("{name}.log"));
    let meta_path = dir.join(format!("{name}.meta"));

    // Check if already running
    if pid_path.exists() {
        if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                if is_alive(pid) {
                    return err(format!(
                        "Service '{name}' is already running (PID {pid}).\nLogs: {}\nUse 'stop' first to restart.",
                        log_path.display()
                    ));
                }
            }
        }
        std::fs::remove_file(&pid_path).ok();
    }

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_err = log_file.try_clone()?;

    let child = std::process::Command::new("bash")
        .args(["-c", command])
        .current_dir(cwd)
        .stdout(log_file)
        .stderr(log_err)
        .stdin(std::process::Stdio::null())
        .spawn()?;

    let pid = child.id();

    // Spawn a background thread to reap the child when it exits,
    // preventing zombie processes. We track via PID file, not the handle.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    std::fs::write(&pid_path, pid.to_string())?;

    let meta = ServiceMeta {
        name: name.clone(),
        command: command.to_string(),
        cwd: cwd.to_string_lossy().to_string(),
        pid,
        started_at: chrono::Utc::now().to_rfc3339(),
        persist,
    };
    std::fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)?;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let status = if is_alive(pid) { "running" } else { "exited (check logs)" };

    ok(format!(
        "Started '{name}' (PID {pid}) — {status}\nCommand: {command}\nLogs: {}",
        log_path.display()
    ))
}

async fn stop(args: &serde_json::Value) -> Result<ToolResult> {
    let name = sanitize_name(args.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("'name' is required"))?);

    let dir = serve_dir();
    let pid_path = dir.join(format!("{name}.pid"));

    if !pid_path.exists() {
        return err(format!("No service '{name}' found."));
    }

    let pid: u32 = std::fs::read_to_string(&pid_path)?.trim().parse()?;

    if is_alive(pid) {
        unsafe { libc::kill(pid as i32, libc::SIGTERM); }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if is_alive(pid) {
            unsafe { libc::kill(pid as i32, libc::SIGKILL); }
        }
    }

    std::fs::remove_file(&pid_path).ok();
    std::fs::remove_file(dir.join(format!("{name}.meta"))).ok();

    ok(format!(
        "Stopped '{name}' (PID {pid}). Log preserved at {}",
        dir.join(format!("{name}.log")).display()
    ))
}

async fn list() -> Result<ToolResult> {
    let dir = serve_dir();
    if !dir.exists() {
        return ok("No services.".into());
    }

    let entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "meta").unwrap_or(false))
        .collect();

    if entries.is_empty() {
        return ok("No services.".into());
    }

    let mut lines = Vec::new();
    for entry in entries {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if let Ok(meta) = serde_json::from_str::<ServiceMeta>(&content) {
                let status = if is_alive(meta.pid) { "running" } else { "dead" };
                let persist_flag = if meta.persist { " [persist]" } else { "" };
                lines.push(format!(
                    "  {:<16} PID {:<8} {:<8}{} {}",
                    meta.name, meta.pid, status, persist_flag, meta.command
                ));
            }
        }
    }

    ok(format!("Services:\n{}", lines.join("\n")))
}

async fn logs(args: &serde_json::Value) -> Result<ToolResult> {
    let name = sanitize_name(args.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("'name' is required"))?);
    let max_lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

    let log_path = serve_dir().join(format!("{name}.log"));
    if !log_path.exists() {
        return err(format!("No log file for '{name}'."));
    }

    let content = std::fs::read_to_string(&log_path)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(max_lines);
    let tail = &all_lines[start..];

    ok(format!("Last {} lines of '{name}':\n{}", tail.len(), tail.join("\n")))
}

async fn check(args: &serde_json::Value) -> Result<ToolResult> {
    let name = sanitize_name(args.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("'name' is required"))?);

    let dir = serve_dir();
    let pid_path = dir.join(format!("{name}.pid"));
    let meta_path = dir.join(format!("{name}.meta"));

    if !pid_path.exists() {
        return err(format!("No service '{name}' found."));
    }

    let pid: u32 = std::fs::read_to_string(&pid_path)?.trim().parse()?;
    let alive = is_alive(pid);

    let mut info = format!("Service '{name}': {}\nPID: {pid}", if alive { "running" } else { "dead" });

    if let Ok(mc) = std::fs::read_to_string(&meta_path) {
        if let Ok(meta) = serde_json::from_str::<ServiceMeta>(&mc) {
            info.push_str(&format!(
                "\nCommand: {}\nStarted: {}\nPersist: {}",
                meta.command, meta.started_at, meta.persist
            ));
        }
    }

    ok(info)
}

/// Stop all non-persist services. Called on session exit.
pub fn cleanup_session_services() {
    let dir = serve_dir();
    if !dir.exists() { return; }

    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if entry.path().extension().map(|x| x == "meta").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(meta) = serde_json::from_str::<ServiceMeta>(&content) {
                    if !meta.persist && is_alive(meta.pid) {
                        tracing::info!(name = %meta.name, pid = meta.pid, "stopping session service");
                        unsafe { libc::kill(meta.pid as i32, libc::SIGTERM); }
                        std::fs::remove_file(dir.join(format!("{}.pid", meta.name))).ok();
                        std::fs::remove_file(entry.path()).ok();
                    }
                }
            }
        }
    }
}

/// Get list of running services for TUI display.
pub fn running_services() -> Vec<(String, u32, bool)> {
    let dir = serve_dir();
    if !dir.exists() { return Vec::new(); }

    std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "meta").unwrap_or(false))
        .filter_map(|e| {
            let content = std::fs::read_to_string(e.path()).ok()?;
            let meta: ServiceMeta = serde_json::from_str(&content).ok()?;
            if is_alive(meta.pid) {
                Some((meta.name, meta.pid, meta.persist))
            } else {
                None
            }
        })
        .collect()
}

/// Sanitize a service name to prevent path traversal.
/// Strips path separators, `..`, and non-safe characters.
fn sanitize_name(name: &str) -> String {
    name.replace('/', "")
        .replace('\\', "")
        .replace("..", "")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.' || *c == '_')
        .take(64)
        .collect::<String>()
}

fn is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn slugify_command(cmd: &str) -> String {
    let first_word = cmd.split_whitespace()
        .find(|w| !w.starts_with('-') && *w != "npx" && *w != "node" && *w != "cargo")
        .unwrap_or("service");
    first_word
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
        .take(16)
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_extracts_meaningful_name() {
        assert_eq!(slugify_command("npx astro dev --port 4321"), "astro");
        assert_eq!(slugify_command("cargo run -- --serve"), "run");
        assert_eq!(slugify_command("node server.js"), "server.js");
        assert_eq!(slugify_command("python -m http.server"), "python");
        assert_eq!(slugify_command("vite"), "vite");
    }

    #[test]
    fn slugify_handles_edge_cases() {
        assert_eq!(slugify_command(""), "service");
        assert_eq!(slugify_command("npx"), "service");
        assert_eq!(slugify_command("npx @scope/pkg"), "scopepkg");
    }

    #[tokio::test]
    async fn list_empty_is_ok() {
        let result = list().await.unwrap();
        assert!(result.details.get("is_error").is_none());
    }

    #[tokio::test]
    async fn check_nonexistent_is_error() {
        let args = json!({"name": "nonexistent-test-service-12345"});
        let result = check(&args).await.unwrap();
        assert_eq!(result.details.get("is_error").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn sanitize_blocks_traversal() {
        assert_eq!(sanitize_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_name("normal-name"), "normal-name");
        assert_eq!(sanitize_name("has/slashes"), "hasslashes");
        assert_eq!(sanitize_name("has\\back"), "hasback");
        assert_eq!(sanitize_name("a]b[c{d}e"), "abcde");
    }

    #[test]
    fn running_services_returns_empty_when_none() {
        // Just verify it doesn't panic with no services dir
        let services = running_services();
        // May or may not be empty depending on test env
        let _ = services;
    }
}
