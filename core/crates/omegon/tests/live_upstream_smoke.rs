use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use tempfile::TempDir;

struct TempRepo {
    _tmp: TempDir,
    cwd: PathBuf,
}

fn live_enabled() -> bool {
    std::env::var("OMEGON_RUN_LIVE_UPSTREAM_TESTS")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn resolve_omegon_binary() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_omegon") {
        return Ok(PathBuf::from(path));
    }

    let current = std::env::current_exe().context("current_exe")?;
    let deps_dir = current.parent().context("integration test executable has no parent")?;
    let debug_dir = deps_dir.parent().context("deps dir has no parent")?;
    let candidate = debug_dir.join(if cfg!(windows) { "omegon.exe" } else { "omegon" });
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
    if !live_enabled() {
        eprintln!("skipping ollama live smoke");
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
