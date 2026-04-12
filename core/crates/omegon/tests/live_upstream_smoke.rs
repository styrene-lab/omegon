use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;
use tempfile::TempDir;

const PROVIDER_CONTRACTS_JSON: &str = include_str!("../../../../.pi/provider-contracts.json");

#[derive(Debug, Deserialize)]
struct ProviderContracts {
    live_upstream_matrix: Vec<LiveUpstreamExpectation>,
}

#[derive(Debug, Deserialize)]
struct LiveUpstreamExpectation {
    provider: String,
    model: String,
    env: Option<String>,
    base_url: String,
    path: String,
    request_format: String,
    response_json_pointer: String,
    reasoning_control: String,
}

struct TempRepo {
    _tmp: TempDir,
    cwd: PathBuf,
}

fn live_enabled() -> bool {
    std::env::var("OMEGON_RUN_LIVE_UPSTREAM_TESTS")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn ollama_local_enabled() -> bool {
    std::env::var("OMEGON_RUN_OLLAMA_LOCAL_LIVE_TEST")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn resolve_omegon_binary() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_omegon") {
        return Ok(PathBuf::from(path));
    }

    let current = std::env::current_exe().context("current_exe")?;
    let deps_dir = current
        .parent()
        .context("integration test executable has no parent")?;
    let debug_dir = deps_dir.parent().context("deps dir has no parent")?;
    let candidate = debug_dir.join(if cfg!(windows) {
        "omegon.exe"
    } else {
        "omegon"
    });
    if candidate.is_file() {
        return Ok(candidate);
    }

    anyhow::bail!(
        "unable to locate omegon binary: CARGO_BIN_EXE_omegon unset and fallback {} missing",
        candidate.display()
    )
}

fn make_repo() -> Result<TempRepo> {
    let tmp = tempfile::tempdir().context("tempdir")?;
    let cwd = tmp.path().to_path_buf();
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(&cwd)
        .status()
        .context("git init")?;
    std::fs::write(cwd.join("README.md"), "live upstream smoke\n").context("write README")?;
    Ok(TempRepo { _tmp: tmp, cwd })
}

fn load_live_upstream_matrix() -> Vec<LiveUpstreamExpectation> {
    serde_json::from_str::<ProviderContracts>(PROVIDER_CONTRACTS_JSON)
        .expect("provider contracts JSON should parse")
        .live_upstream_matrix
}

fn expectation_for(provider: &str) -> LiveUpstreamExpectation {
    load_live_upstream_matrix()
        .into_iter()
        .find(|entry| entry.provider == provider)
        .unwrap_or_else(|| panic!("missing live upstream matrix entry for {provider}"))
}

fn run_prompt(model: &str) -> Result<String> {
    let repo = make_repo()?;
    let bin = resolve_omegon_binary()?;
    let output = Command::new(bin)
        .arg("--model")
        .arg(model)
        .arg("--prompt")
        .arg("Reply with exactly OK")
        .current_dir(&repo.cwd)
        .env("NO_COLOR", "1")
        .output()
        .with_context(|| format!("run omegon --prompt for model {model}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "prompt run failed for {model}: status={} stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[test]
fn live_upstream_suite_is_opt_in() {
    if !live_enabled() {
        eprintln!("live upstream tests disabled; set OMEGON_RUN_LIVE_UPSTREAM_TESTS=1 to enable");
    }
}

#[test]
fn provider_contract_matrix_parses_and_covers_expected_providers() {
    let matrix = load_live_upstream_matrix();
    assert_eq!(matrix.len(), 4);
    let providers: Vec<&str> = matrix.iter().map(|entry| entry.provider.as_str()).collect();
    assert_eq!(
        providers,
        vec!["anthropic", "openai", "ollama", "ollama-cloud"]
    );
}

#[test]
fn provider_contract_matrix_uses_expected_endpoint_shapes() {
    let anthropic = expectation_for("anthropic");
    assert_eq!(anthropic.model, "claude-sonnet-4-6");
    assert_eq!(anthropic.env.as_deref(), Some("ANTHROPIC_API_KEY"));
    assert_eq!(anthropic.base_url, "https://api.anthropic.com");
    assert_eq!(anthropic.path, "/v1/messages");
    assert_eq!(anthropic.request_format, "anthropic_messages");
    assert_eq!(anthropic.response_json_pointer, "/content/0/text");
    assert_eq!(anthropic.reasoning_control, "thinking");

    let openai = expectation_for("openai");
    assert_eq!(openai.model, "gpt-5.4");
    assert_eq!(openai.env.as_deref(), Some("OPENAI_API_KEY"));
    assert_eq!(openai.base_url, "https://api.openai.com");
    assert_eq!(openai.path, "/v1/chat/completions");
    assert_eq!(openai.request_format, "openai_chat_completions");
    assert_eq!(openai.response_json_pointer, "/choices/0/message/content");
    assert_eq!(openai.reasoning_control, "reasoning");

    let ollama = expectation_for("ollama");
    assert_eq!(ollama.model, "qwen3:32b");
    assert_eq!(ollama.env, None);
    assert_eq!(ollama.base_url, "http://localhost:11434");
    assert_eq!(ollama.path, "/api/chat");
    assert_eq!(ollama.request_format, "ollama_chat");
    assert_eq!(ollama.response_json_pointer, "/message/content");
    assert_eq!(ollama.reasoning_control, "think");

    let ollama_cloud = expectation_for("ollama-cloud");
    assert_eq!(ollama_cloud.model, "gpt-oss:120b-cloud");
    assert_eq!(ollama_cloud.env.as_deref(), Some("OLLAMA_API_KEY"));
    assert_eq!(ollama_cloud.base_url, "https://ollama.com/api");
    assert_eq!(ollama_cloud.path, "/api/chat");
    assert_eq!(ollama_cloud.request_format, "ollama_chat");
    assert_eq!(ollama_cloud.response_json_pointer, "/message/content");
    assert_eq!(ollama_cloud.reasoning_control, "think");
}

#[test]
fn live_anthropic_prompt_round_trip() -> Result<()> {
    if !live_enabled() || std::env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!("skipping anthropic live smoke");
        return Ok(());
    }
    let out = run_prompt("anthropic:claude-sonnet-4-6")?;
    assert!(out.contains("OK"), "unexpected anthropic output: {out}");
    Ok(())
}

#[test]
fn live_openai_prompt_round_trip() -> Result<()> {
    if !live_enabled() || std::env::var("OPENAI_API_KEY").is_err() {
        eprintln!("skipping openai live smoke");
        return Ok(());
    }
    let out = run_prompt("openai:gpt-5.4")?;
    assert!(out.contains("OK"), "unexpected openai output: {out}");
    Ok(())
}

#[test]
fn live_ollama_local_prompt_round_trip() -> Result<()> {
    if !live_enabled() || !ollama_local_enabled() {
        eprintln!("skipping ollama local live smoke");
        return Ok(());
    }
    let out = run_prompt("qwen3:32b")?;
    assert!(out.contains("OK"), "unexpected ollama output: {out}");
    Ok(())
}

#[test]
fn live_ollama_cloud_prompt_round_trip() -> Result<()> {
    if !live_enabled() || std::env::var("OLLAMA_API_KEY").is_err() {
        eprintln!("skipping ollama-cloud live smoke");
        return Ok(());
    }
    let out = run_prompt("ollama-cloud:gpt-oss:120b-cloud")?;
    assert!(out.contains("OK"), "unexpected ollama-cloud output: {out}");
    Ok(())
}
