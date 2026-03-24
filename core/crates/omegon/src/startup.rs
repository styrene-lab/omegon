//! Startup systems check — probe the environment to discover capabilities.
//!
//! Each probe runs independently and sends its result through a channel.
//! The splash screen receives results via `try_recv()` each frame and
//! updates the checklist grid. After all probes complete, results are
//! classified into a `CapabilityTier` for tutorial and routing decisions.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

// ─── Types ──────────────────────────────────────────────────────────────────

/// Result of a single startup probe.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub label: &'static str,
    pub state: ProbeState,
    pub summary: String,
}

/// Outcome of a probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeState {
    Done,
    Failed,
}

/// Capability tier derived from probe results. Drives tutorial variant
/// selection, default routing policy, and bootstrap panel messaging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityTier {
    /// Anthropic or OpenAI API key present. Full experience.
    FullCloud,
    /// Ollama running with 14B+ model, 32GB+ RAM. Full experience, local.
    BeefyLocal,
    /// OpenRouter or other free cloud API key present.
    FreeCloud,
    /// Ollama with small model (4B-8B). Abbreviated experience.
    SmallLocal,
    /// Nothing available. UI tour only.
    Offline,
}

// ─── Probe orchestrator ─────────────────────────────────────────────────────

/// Run all startup probes in parallel and send results through `tx`.
/// Each probe sends its result independently as it completes.
/// The entire function completes within 2 seconds even if endpoints are unreachable.
pub async fn run_probes(tx: mpsc::Sender<ProbeResult>, cwd: String) {
    let tx1 = tx.clone();
    let tx2 = tx.clone();
    let tx3 = tx.clone();
    let tx4 = tx.clone();
    let tx5 = tx.clone();
    let tx6 = tx.clone();
    let tx7 = tx.clone();
    let tx8 = tx.clone();
    let tx9 = tx;
    let cwd2 = cwd.clone();
    let cwd3 = cwd.clone();

    // Fire all probes concurrently. Each sends its result as it completes.
    // The tokio::time::timeout wraps the entire join to enforce the 2s ceiling.
    //
    // Fast probes (env checks, file reads) complete in <1ms. To produce a
    // visible cascade in the splash grid, we stagger sends with small delays.
    // The total stagger is ~400ms — well within the 1.7s animation window.
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            // Network probes run immediately (they have real latency)
            async { let _ = tx2.send(probe_local().await); },
            // Fast probes stagger at ~50ms intervals for visual cascade
            async {
                let _ = tx1.send(probe_cloud());
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = tx3.send(probe_hardware());
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = tx4.send(probe_memory(&cwd));
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = tx5.send(probe_tools());
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = tx6.send(probe_design(&cwd2));
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = tx7.send(probe_secrets());
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = tx8.send(probe_container());
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = tx9.send(probe_mcp(&cwd3));
            },
        )
    }).await;
}

/// Classify probe results into a capability tier.
pub fn classify_tier(results: &[ProbeResult]) -> CapabilityTier {
    let cloud = results.iter().find(|r| r.label == "cloud");
    let local = results.iter().find(|r| r.label == "local");
    let hw = results.iter().find(|r| r.label == "hardware");

    // Full cloud: any major cloud provider key present
    if let Some(r) = cloud {
        if r.state == ProbeState::Done && (r.summary.contains("anthropic") || r.summary.contains("openai")) {
            return CapabilityTier::FullCloud;
        }
    }

    // Beefy local: Ollama with models + sufficient RAM
    let has_good_local = local
        .is_some_and(|r| r.state == ProbeState::Done && !r.summary.contains("no models"));
    let has_beefy_hw = hw
        .is_some_and(|r| r.state == ProbeState::Done && (
            r.summary.contains("32GB") || r.summary.contains("64GB")
            || r.summary.contains("96GB") || r.summary.contains("128GB")
            || r.summary.contains("192GB")));

    if has_good_local && has_beefy_hw {
        return CapabilityTier::BeefyLocal;
    }

    // Free cloud: OpenRouter key present
    if let Some(r) = cloud {
        if r.state == ProbeState::Done && r.summary.contains("openrouter") {
            return CapabilityTier::FreeCloud;
        }
    }

    // Small local: Ollama running with any model
    if has_good_local {
        return CapabilityTier::SmallLocal;
    }

    CapabilityTier::Offline
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Run a subprocess with a timeout. Returns None if it times out or fails to spawn.
fn timed_command(cmd: &str, args: &[&str], timeout_ms: u64) -> Option<std::process::Output> {
    use std::process::Command;
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

// ─── Individual probes ──────────────────────────────────────────────────────

fn probe_cloud() -> ProbeResult {
    let mut providers = Vec::new();

    // Check env vars
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        providers.push("anthropic");
    }
    if std::env::var("OPENAI_API_KEY").is_ok() {
        providers.push("openai");
    }
    if std::env::var("OPENROUTER_API_KEY").is_ok() {
        providers.push("openrouter");
    }

    // Also check stored credentials using canonical provider map
    for p in crate::auth::PROVIDERS {
        if matches!(p.auth_method, crate::auth::AuthMethod::OAuth | crate::auth::AuthMethod::ApiKey) {
            if crate::auth::read_credentials(p.auth_key)
                .is_some_and(|c| !c.access.is_empty())
            {
                if !providers.contains(&p.id) {
                    providers.push(p.id);
                }
            }
        }
    }

    if providers.is_empty() {
        ProbeResult { label: "cloud", state: ProbeState::Failed, summary: "none".into() }
    } else {
        ProbeResult { label: "cloud", state: ProbeState::Done, summary: providers.join(", ") }
    }
}

async fn probe_local() -> ProbeResult {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(100))
        .timeout(Duration::from_millis(300))
        .build()
        .unwrap_or_default();

    // Probe all ports in parallel
    let c1 = client.clone();
    let c2 = client.clone();
    let c3 = client;

    let (ollama, lmstudio, vllm) = tokio::join!(
        async {
            match c1.get("http://localhost:11434/api/tags").send().await {
                Ok(resp) => match resp.text().await {
                    Ok(body) => {
                        let count = body.matches("\"name\"").count();
                        if count > 0 { Some(format!("ollama: {count}")) }
                        else { Some("ollama: no models".into()) }
                    }
                    Err(_) => None,
                },
                Err(_) => None,
            }
        },
        async {
            c2.get("http://localhost:1234/v1/models").send().await
                .ok().filter(|r| r.status().is_success()).map(|_| "lmstudio".to_string())
        },
        async {
            c3.get("http://localhost:8080/v1/models").send().await
                .ok().filter(|r| r.status().is_success()).map(|_| "vllm".to_string())
        },
    );

    let found: Vec<String> = [ollama, lmstudio, vllm].into_iter().flatten().collect();

    if found.is_empty() {
        ProbeResult { label: "local", state: ProbeState::Failed, summary: "not found".into() }
    } else {
        ProbeResult { label: "local", state: ProbeState::Done, summary: found.join(", ") }
    }
}

fn probe_hardware() -> ProbeResult {
    let mut parts = Vec::new();

    #[cfg(target_os = "macos")]
    {
        // Detect chip + RAM in one pass (single sysctl call already captured brand)
        if let Some(out) = timed_command("sysctl", &["-n", "machdep.cpu.brand_string"], 500) {
            let brand = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if brand.contains("Apple") {
                let name = brand.strip_prefix("Apple ").unwrap_or(&brand);
                parts.push(name.to_string());
            }
        }

        // RAM via sysctl
        if let Some(out) = timed_command("sysctl", &["-n", "hw.memsize"], 500) {
            if let Ok(bytes) = String::from_utf8_lossy(&out.stdout).trim().parse::<u64>() {
                let gb = bytes / (1024 * 1024 * 1024);
                parts.push(format!("{gb}GB"));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // GPU via nvidia-smi (500ms timeout — nvidia-smi can be slow)
        if let Some(out) = timed_command("nvidia-smi",
            &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"], 500)
        {
            if out.status.success() {
                let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Some((name, vram)) = line.split_once(',') {
                    parts.push(format!("{}, {}MB VRAM", name.trim(), vram.trim()));
                }
            }
        }

        // RAM via /proc/meminfo (no subprocess needed)
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            if let Some(line) = content.lines().find(|l| l.starts_with("MemTotal:")) {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<u64>() {
                        let gb = kb / (1024 * 1024);
                        parts.push(format!("{gb}GB"));
                    }
                }
            }
        }
    }

    if parts.is_empty() {
        parts.push(std::env::consts::ARCH.to_string());
    }

    ProbeResult {
        label: "hardware",
        state: ProbeState::Done,
        summary: parts.join(", "),
    }
}

fn probe_memory(cwd: &str) -> ProbeResult {
    // Check for facts.jsonl
    let facts_paths = [
        Path::new(cwd).join(".pi/memory/facts.jsonl"),
        Path::new(cwd).join("ai/memory/facts.jsonl"),
    ];

    for path in &facts_paths {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                let count = content.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count();
                if count > 0 {
                    return ProbeResult {
                        label: "memory",
                        state: ProbeState::Done,
                        summary: format!("{count} facts"),
                    };
                }
            }
        }
    }

    ProbeResult { label: "memory", state: ProbeState::Done, summary: "empty".into() }
}

fn probe_tools() -> ProbeResult {
    // Count from the static tool registry
    let count = crate::tool_registry::TOOL_COUNT;
    ProbeResult {
        label: "tools",
        state: ProbeState::Done,
        summary: format!("{count} registered"),
    }
}

fn probe_design(cwd: &str) -> ProbeResult {
    let docs_dir = Path::new(cwd).join("docs");
    if !docs_dir.is_dir() {
        return ProbeResult { label: "design", state: ProbeState::Done, summary: "empty".into() };
    }

    // Count .md files that have design-node frontmatter (id: field)
    let count = std::fs::read_dir(&docs_dir)
        .map(|entries| entries.filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().is_some_and(|ext| ext == "md")
                    && std::fs::read_to_string(e.path())
                        .is_ok_and(|c| c.starts_with("---") && c.contains("\nid:"))
            })
            .count())
        .unwrap_or(0);

    ProbeResult {
        label: "design",
        state: ProbeState::Done,
        summary: if count > 0 { format!("{count} nodes") } else { "empty".into() },
    }
}

fn probe_secrets() -> ProbeResult {
    // Check vault availability with timeout (vault status can hang)
    if timed_command("vault", &["version"], 500).is_some() {
        ProbeResult { label: "secrets", state: ProbeState::Done, summary: "vault".into() }
    } else {
        // No vault — report keyring as fallback (always available on macOS/Linux)
        ProbeResult { label: "secrets", state: ProbeState::Done, summary: "keyring".into() }
    }
}

fn probe_container() -> ProbeResult {
    // Try podman first, then docker — with timeout (Docker Desktop can be slow)
    for (cmd, name) in &[("podman", "podman"), ("docker", "docker")] {
        if let Some(out) = timed_command(cmd, &["--version"], 1000) {
            if out.status.success() {
                let ver = String::from_utf8_lossy(&out.stdout);
                let version = ver.split_whitespace()
                    .find(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()))
                    .unwrap_or("unknown");
                return ProbeResult {
                    label: "container",
                    state: ProbeState::Done,
                    summary: format!("{name} {version}"),
                };
            }
        }
    }

    ProbeResult { label: "container", state: ProbeState::Failed, summary: "not found".into() }
}

fn probe_mcp(cwd: &str) -> ProbeResult {
    // Count MCP server configs from plugin manifests
    let plugin_dir = Path::new(cwd).join(".omegon/plugins");
    if !plugin_dir.is_dir() {
        return ProbeResult { label: "mcp", state: ProbeState::Done, summary: "none".into() };
    }

    // Simple: count TOML files that contain [mcp]
    let count = std::fs::read_dir(&plugin_dir)
        .map(|entries| entries.filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().is_some_and(|ext| ext == "toml")
                    && std::fs::read_to_string(e.path())
                        .is_ok_and(|c| c.contains("[mcp"))
            })
            .count())
        .unwrap_or(0);

    if count > 0 {
        ProbeResult {
            label: "mcp",
            state: ProbeState::Done,
            summary: format!("{count} server{}", if count == 1 { "" } else { "s" }),
        }
    } else {
        ProbeResult { label: "mcp", state: ProbeState::Done, summary: "none".into() }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_cloud_checks_env() {
        // This test runs in whatever env CI/dev has — just ensure no panic
        let result = probe_cloud();
        assert_eq!(result.label, "cloud");
        assert!(!result.summary.is_empty());
    }

    #[test]
    fn probe_hardware_doesnt_panic() {
        let result = probe_hardware();
        assert_eq!(result.label, "hardware");
        assert_eq!(result.state, ProbeState::Done);
        assert!(!result.summary.is_empty());
    }

    #[test]
    fn probe_memory_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = probe_memory(tmp.path().to_str().unwrap());
        assert_eq!(result.label, "memory");
        assert_eq!(result.summary, "empty");
    }

    #[test]
    fn probe_memory_with_facts() {
        let tmp = tempfile::TempDir::new().unwrap();
        let pi_dir = tmp.path().join(".pi/memory");
        std::fs::create_dir_all(&pi_dir).unwrap();
        std::fs::write(pi_dir.join("facts.jsonl"), "{\"id\":\"1\"}\n{\"id\":\"2\"}\n").unwrap();
        let result = probe_memory(tmp.path().to_str().unwrap());
        assert_eq!(result.summary, "2 facts");
    }

    #[test]
    fn probe_design_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = probe_design(tmp.path().to_str().unwrap());
        assert_eq!(result.summary, "empty");
    }

    #[test]
    fn probe_design_with_nodes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        // Real design nodes have frontmatter with id:
        std::fs::write(docs.join("node-a.md"), "---\nid: node-a\ntitle: A\n---\n# A").unwrap();
        std::fs::write(docs.join("node-b.md"), "---\nid: node-b\ntitle: B\n---\n# B").unwrap();
        // These should NOT count
        std::fs::write(docs.join("readme.md"), "# Just a readme").unwrap();
        std::fs::write(docs.join("readme.txt"), "not md").unwrap();
        let result = probe_design(tmp.path().to_str().unwrap());
        assert_eq!(result.summary, "2 nodes");
    }

    #[test]
    fn classify_tier_full_cloud() {
        let results = vec![
            ProbeResult { label: "cloud", state: ProbeState::Done, summary: "anthropic, openai".into() },
            ProbeResult { label: "local", state: ProbeState::Failed, summary: "not found".into() },
            ProbeResult { label: "hardware", state: ProbeState::Done, summary: "M2 Pro, 32GB".into() },
        ];
        assert_eq!(classify_tier(&results), CapabilityTier::FullCloud);
    }

    #[test]
    fn classify_tier_beefy_local() {
        let results = vec![
            ProbeResult { label: "cloud", state: ProbeState::Failed, summary: "none".into() },
            ProbeResult { label: "local", state: ProbeState::Done, summary: "ollama: 7".into() },
            ProbeResult { label: "hardware", state: ProbeState::Done, summary: "M2 Pro, 32GB".into() },
        ];
        assert_eq!(classify_tier(&results), CapabilityTier::BeefyLocal);
    }

    #[test]
    fn classify_tier_free_cloud() {
        let results = vec![
            ProbeResult { label: "cloud", state: ProbeState::Done, summary: "openrouter".into() },
            ProbeResult { label: "local", state: ProbeState::Failed, summary: "not found".into() },
            ProbeResult { label: "hardware", state: ProbeState::Done, summary: "16GB".into() },
        ];
        assert_eq!(classify_tier(&results), CapabilityTier::FreeCloud);
    }

    #[test]
    fn classify_tier_small_local() {
        let results = vec![
            ProbeResult { label: "cloud", state: ProbeState::Failed, summary: "none".into() },
            ProbeResult { label: "local", state: ProbeState::Done, summary: "ollama: 1".into() },
            ProbeResult { label: "hardware", state: ProbeState::Done, summary: "16GB".into() },
        ];
        assert_eq!(classify_tier(&results), CapabilityTier::SmallLocal);
    }

    #[test]
    fn classify_tier_offline() {
        let results = vec![
            ProbeResult { label: "cloud", state: ProbeState::Failed, summary: "none".into() },
            ProbeResult { label: "local", state: ProbeState::Failed, summary: "not found".into() },
            ProbeResult { label: "hardware", state: ProbeState::Done, summary: "8GB".into() },
        ];
        assert_eq!(classify_tier(&results), CapabilityTier::Offline);
    }
}
