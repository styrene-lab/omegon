//! whoami tool — check authentication status across dev tools.
//!
//! Shells out to git, gh, glab, aws, kubectl, podman/docker, vault
//! and parses output to report auth status.

use omegon_traits::{ContentBlock, ToolResult};
use serde_json::{Value, json};
use std::process::Command;

#[derive(Debug, Clone, Copy)]
enum Status { Ok, Expired, Invalid, None, Missing }

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Expired => "expired",
            Self::Invalid => "invalid",
            Self::None => "none",
            Self::Missing => "missing",
        }
    }
    fn icon(&self) -> &'static str {
        match self {
            Self::Ok => "✓",
            Self::Expired => "⚠",
            Self::Invalid | Self::None => "✗",
            Self::Missing => "·",
        }
    }
}

struct AuthResult {
    provider: String,
    status: Status,
    detail: String,
    error: Option<String>,
    refresh: Option<String>,
}

pub async fn execute() -> anyhow::Result<ToolResult> {
    let results = tokio::task::spawn_blocking(check_all).await?;

    let mut lines = vec!["**Auth Status**".to_string(), String::new()];
    let mut checks = Vec::new();

    for r in &results {
        let icon = r.status.icon();
        let mut line = format!("  {icon}  **{}**: {}", r.provider, r.detail);
        if let Some(ref err) = r.error {
            if !matches!(r.status, Status::Ok) {
                let first_line = err.lines().next().unwrap_or("").chars().take(120).collect::<String>();
                line.push_str(&format!("\n      Error: {first_line}"));
            }
        }
        lines.push(line);

        checks.push(json!({
            "provider": r.provider,
            "status": r.status.as_str(),
            "detail": r.detail,
            "error": r.error,
        }));
    }

    // Actionable items
    let fixable: Vec<_> = results.iter()
        .filter(|r| matches!(r.status, Status::Expired | Status::Invalid | Status::None))
        .collect();
    if !fixable.is_empty() {
        lines.push(String::new());
        lines.push("**To fix:**".into());
        for r in fixable {
            if let Some(ref refresh) = r.refresh {
                lines.push(format!("  {}: `{refresh}`", r.provider));
            }
        }
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text: lines.join("\n") }],
        details: json!({ "checks": checks }),
    })
}

fn check_all() -> Vec<AuthResult> {
    vec![
        check_git(),
        check_github(),
        check_gitlab(),
        check_aws(),
        check_kubernetes(),
        check_oci(),
        check_vault(),
    ]
}

fn has_cmd(name: &str) -> bool {
    Command::new("which").arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status().is_ok_and(|s| s.success())
}

fn run_cmd(cmd: &str, args: &[&str]) -> (bool, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(out) => (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        ),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

fn diagnose_error(stderr: &str) -> (Status, String) {
    let lower = stderr.to_lowercase();
    if lower.contains("expired") || lower.contains("expiredtoken") {
        return (Status::Expired, "Token or session has expired".into());
    }
    if lower.contains("not logged") || lower.contains("no token")
        || lower.contains("not authenticated") || lower.contains("login required") {
        return (Status::None, "Not authenticated".into());
    }
    if lower.contains("bad credentials") || lower.contains("authentication failed")
        || lower.contains("401") || lower.contains("unauthorized") {
        return (Status::Invalid, extract_error_line(stderr));
    }
    (Status::None, extract_error_line(stderr))
}

fn extract_error_line(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.trim().lines().filter(|l| !l.trim().is_empty()).collect();
    let err_line = lines.iter().find(|l| {
        let lower = l.to_lowercase();
        lower.contains("error") || lower.contains("failed") || lower.contains("expired")
            || lower.contains("denied") || lower.contains("401") || lower.contains("403")
    });
    let s = err_line.unwrap_or(lines.first().unwrap_or(&"Unknown error"));
    s.trim().chars().take(200).collect()
}

// ─── Providers ───────────────────────────────────────────────────

fn check_git() -> AuthResult {
    let (_, name, _) = run_cmd("git", &["config", "user.name"]);
    let (_, email, _) = run_cmd("git", &["config", "user.email"]);
    let name = name.trim().to_string();
    let email = email.trim().to_string();

    if !name.is_empty() && !email.is_empty() {
        AuthResult { provider: "git".into(), status: Status::Ok, detail: format!("{name} <{email}>"), error: None, refresh: None }
    } else {
        AuthResult {
            provider: "git".into(), status: Status::None,
            detail: format!("name: {}, email: {}", if name.is_empty() { "(not set)" } else { &name }, if email.is_empty() { "(not set)" } else { &email }),
            error: None, refresh: Some("git config --global user.name \"...\" && git config --global user.email \"...\"".into()),
        }
    }
}

fn check_github() -> AuthResult {
    if !has_cmd("gh") {
        return AuthResult { provider: "github".into(), status: Status::Missing, detail: "gh CLI not installed".into(), error: None, refresh: None };
    }
    let (ok, stdout, stderr) = run_cmd("gh", &["auth", "status"]);
    let output = format!("{stdout}\n{stderr}").trim().to_string();
    if ok {
        let detail = if let Some(cap) = output.lines().find(|l| l.contains("Logged in to")) {
            cap.trim().to_string()
        } else { "authenticated".into() };
        AuthResult { provider: "github".into(), status: Status::Ok, detail, error: None, refresh: Some("gh auth login".into()) }
    } else {
        let (status, reason) = diagnose_error(&output);
        AuthResult { provider: "github".into(), status, detail: reason, error: Some(output.chars().take(300).collect()), refresh: Some("gh auth login".into()) }
    }
}

fn check_gitlab() -> AuthResult {
    if !has_cmd("glab") {
        if std::env::var("GITLAB_TOKEN").is_ok_and(|v| !v.is_empty()) {
            return AuthResult { provider: "gitlab".into(), status: Status::Ok, detail: "GITLAB_TOKEN set (glab CLI not installed)".into(), error: None, refresh: None };
        }
        return AuthResult { provider: "gitlab".into(), status: Status::Missing, detail: "glab CLI not installed".into(), error: None, refresh: None };
    }
    let (ok, stdout, stderr) = run_cmd("glab", &["auth", "status"]);
    let output = format!("{stdout}\n{stderr}").trim().to_string();
    if ok {
        let detail = output.lines().find(|l| l.contains("Logged in")).map(|l| l.trim().to_string()).unwrap_or("authenticated".into());
        AuthResult { provider: "gitlab".into(), status: Status::Ok, detail, error: None, refresh: Some("glab auth login".into()) }
    } else {
        let (status, reason) = diagnose_error(&output);
        AuthResult { provider: "gitlab".into(), status, detail: reason, error: Some(output.chars().take(300).collect()), refresh: Some("glab auth login".into()) }
    }
}

fn check_aws() -> AuthResult {
    if !has_cmd("aws") {
        return AuthResult { provider: "aws".into(), status: Status::Missing, detail: "aws CLI not installed".into(), error: None, refresh: None };
    }
    let (ok, stdout, stderr) = run_cmd("aws", &["sts", "get-caller-identity", "--output", "json"]);
    if ok {
        let detail = serde_json::from_str::<Value>(&stdout).ok()
            .and_then(|v| v["Arn"].as_str().map(String::from))
            .unwrap_or("authenticated".into());
        AuthResult { provider: "aws".into(), status: Status::Ok, detail, error: None, refresh: Some("aws sso login".into()) }
    } else {
        let (status, reason) = diagnose_error(&stderr);
        AuthResult { provider: "aws".into(), status, detail: reason, error: Some(stderr.chars().take(300).collect()), refresh: Some("aws sso login".into()) }
    }
}

fn check_kubernetes() -> AuthResult {
    if !has_cmd("kubectl") {
        return AuthResult { provider: "kubernetes".into(), status: Status::Missing, detail: "kubectl not installed".into(), error: None, refresh: None };
    }
    let (ok, stdout, _) = run_cmd("kubectl", &["config", "current-context"]);
    if !ok {
        return AuthResult { provider: "kubernetes".into(), status: Status::None, detail: "No context set".into(), error: None, refresh: Some("kubectl config use-context <ctx>".into()) };
    }
    let context = stdout.trim().to_string();
    let (ok2, _, stderr2) = run_cmd("kubectl", &["cluster-info", "--request-timeout=5s"]);
    if ok2 {
        AuthResult { provider: "kubernetes".into(), status: Status::Ok, detail: format!("context: {context}"), error: None, refresh: None }
    } else {
        let (status, reason) = diagnose_error(&stderr2);
        AuthResult { provider: "kubernetes".into(), status, detail: format!("context: {context} — {reason}"), error: Some(stderr2.chars().take(300).collect()), refresh: None }
    }
}

fn check_oci() -> AuthResult {
    let cmd = if has_cmd("podman") { "podman" } else if has_cmd("docker") { "docker" } else {
        return AuthResult { provider: "oci".into(), status: Status::Missing, detail: "Neither podman nor docker installed".into(), error: None, refresh: None };
    };
    let (ok, stdout, _) = run_cmd(cmd, &["login", "--get-login", "ghcr.io"]);
    if ok {
        AuthResult { provider: "oci".into(), status: Status::Ok, detail: format!("ghcr.io: {} ({cmd})", stdout.trim()), error: None, refresh: None }
    } else {
        AuthResult { provider: "oci".into(), status: Status::None, detail: format!("Not logged in to ghcr.io ({cmd})"), error: None,
            refresh: Some(format!("gh auth token | {cmd} login ghcr.io -u $(gh api user --jq .login) --password-stdin")) }
    }
}

fn check_vault() -> AuthResult {
    if !has_cmd("vault") {
        return AuthResult { provider: "vault".into(), status: Status::Missing, detail: "vault CLI not installed".into(), error: None, refresh: None };
    }
    if std::env::var("VAULT_ADDR").is_err() {
        return AuthResult { provider: "vault".into(), status: Status::None, detail: "VAULT_ADDR not set".into(), error: None, refresh: Some("vault login".into()) };
    }
    let (ok, stdout, stderr) = run_cmd("vault", &["token", "lookup", "-format=json"]);
    if ok {
        let detail = serde_json::from_str::<Value>(&stdout).ok()
            .and_then(|v| {
                let data = v.get("data")?;
                let name = data["display_name"].as_str().unwrap_or("");
                let expire = data["expire_time"].as_str().map(|e| e.split('T').next().unwrap_or(e)).unwrap_or("no expiry");
                Some(format!("{name} · expires: {expire}"))
            })
            .unwrap_or("authenticated".into());
        AuthResult { provider: "vault".into(), status: Status::Ok, detail, error: None, refresh: Some("vault login".into()) }
    } else {
        let output = format!("{stdout}\n{stderr}");
        let (status, reason) = diagnose_error(&output);
        AuthResult { provider: "vault".into(), status, detail: reason, error: Some(output.chars().take(300).collect()), refresh: Some("vault login".into()) }
    }
}
