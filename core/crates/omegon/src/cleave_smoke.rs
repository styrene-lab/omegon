use crate::Cli;
use crate::cleave;
use anyhow::Context;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Deterministic cleave smoke scenarios.
///
/// Each scenario injects a child outcome via `OMEGON_CLEAVE_SMOKE_CHILD_MODE`,
/// runs the orchestrator against a disposable git repo, and asserts the
/// resulting status summary and merge-result line are correct.
///
/// Adding a new regression:
/// 1. Add a `SmokeScenario` entry below.
/// 2. Describe `child_mode` — either an existing mode or add a new one to
///    `maybe_run_injected_cleave_smoke_child()` in `main.rs`.
/// 3. Set the expected `status_line` and `merge_line` substrings.
///
/// Child modes (from `maybe_run_injected_cleave_smoke_child`):
///   upstream-exhausted  — exits 2 (provider exhaustion)
///   fail                — exits 1 (logic failure)
///   success-noop        — exits 0, no file writes
///   success-dirty       — exits 0, writes OMEGON_CLEAVE_SMOKE_WRITE_FILE
struct SmokeScenario {
    name: &'static str,
    child_mode: &'static str,
    write_file: Option<&'static str>,
    expect_exit_ok: bool,
    expect_status_line: &'static str,
    expect_merge_line: &'static str,
    runtime_profile: Option<crate::cleave::CleaveChildRuntimeProfile>,
    assert_runtime: Option<Box<dyn Fn(&serde_json::Value) -> anyhow::Result<()> + Send + Sync>>,
}

fn runtime_matrix_assert(
    expected_model: &'static str,
    expected_provider: &'static str,
) -> Box<dyn Fn(&serde_json::Value) -> anyhow::Result<()> + Send + Sync> {
    Box::new(move |report| {
        if report["model"] != expected_model {
            anyhow::bail!("model mismatch: expected {expected_model}, got {report}");
        }
        if report["provider"] != expected_provider {
            anyhow::bail!("provider mismatch: expected {expected_provider}, got {report}");
        }
        Ok(())
    })
}

pub async fn run(cli: &Cli) -> anyhow::Result<()> {
    let scenarios = vec![
        SmokeScenario {
            name: "upstream_exhausted_no_changes",
            child_mode: "upstream-exhausted",
            write_file: None,
            expect_exit_ok: false,
            expect_status_line: "0 completed, 0 failed, 1 upstream exhausted, 0 unfinished",
            expect_merge_line: "upstream exhausted (no repo changes to merge)",
            runtime_profile: None,
            assert_runtime: None,
        },
        SmokeScenario {
            name: "failed_no_changes",
            child_mode: "fail",
            write_file: None,
            expect_exit_ok: false,
            expect_status_line: "0 completed, 1 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "failed (no repo changes to merge)",
            runtime_profile: None,
            assert_runtime: None,
        },
        SmokeScenario {
            name: "completed_no_changes",
            child_mode: "success-noop",
            write_file: None,
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "completed (no changes)",
            runtime_profile: None,
            assert_runtime: None,
        },
        SmokeScenario {
            name: "completed_with_merge",
            child_mode: "success-dirty",
            write_file: Some("README.md"),
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "merged",
            runtime_profile: None,
            assert_runtime: None,
        },
        SmokeScenario {
            name: "runtime_profile_enforced",
            child_mode: "report-runtime",
            write_file: None,
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "completed (no changes)",
            runtime_profile: Some(crate::cleave::CleaveChildRuntimeProfile {
                model: Some("anthropic:claude-sonnet-4-6".into()),
                thinking_level: Some("high".into()),
                context_class: Some("legion".into()),
                disabled_tools: vec!["bash".into()],
                skills: vec!["security".into()],
                enabled_extensions: vec!["alpha".into()],
                disabled_extensions: vec!["beta".into()],
                preloaded_files: vec!["docs/runtime-preload.md".into()],
                ..Default::default()
            }),
            assert_runtime: Some(Box::new(assert_runtime_profile_report)),
        },
        SmokeScenario {
            name: "runtime_model_matrix_claude",
            child_mode: "report-runtime",
            write_file: None,
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "completed (no changes)",
            runtime_profile: Some(crate::cleave::CleaveChildRuntimeProfile {
                model: Some("anthropic:claude-sonnet-4-6".into()),
                ..Default::default()
            }),
            assert_runtime: Some(runtime_matrix_assert("claude-sonnet-4-6", "anthropic")),
        },
        SmokeScenario {
            name: "runtime_model_matrix_openai_gpt54",
            child_mode: "report-runtime",
            write_file: None,
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "completed (no changes)",
            runtime_profile: Some(crate::cleave::CleaveChildRuntimeProfile {
                model: Some("openai:gpt-5.4".into()),
                ..Default::default()
            }),
            assert_runtime: Some(runtime_matrix_assert("gpt-5.4", "openai")),
        },
        SmokeScenario {
            name: "runtime_model_matrix_openai_gpt41",
            child_mode: "report-runtime",
            write_file: None,
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "completed (no changes)",
            runtime_profile: Some(crate::cleave::CleaveChildRuntimeProfile {
                model: Some("openai:gpt-4.1".into()),
                ..Default::default()
            }),
            assert_runtime: Some(runtime_matrix_assert("gpt-4.1", "openai")),
        },
        SmokeScenario {
            name: "runtime_model_matrix_ollama_local",
            child_mode: "report-runtime",
            write_file: None,
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "completed (no changes)",
            runtime_profile: Some(crate::cleave::CleaveChildRuntimeProfile {
                model: Some("qwen3:32b".into()),
                ..Default::default()
            }),
            assert_runtime: Some(runtime_matrix_assert("qwen3:32b", "ollama")),
        },
        SmokeScenario {
            name: "runtime_model_matrix_local_alias",
            child_mode: "report-runtime",
            write_file: None,
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "completed (no changes)",
            runtime_profile: Some(crate::cleave::CleaveChildRuntimeProfile {
                model: Some("local:qwen3:30b".into()),
                ..Default::default()
            }),
            assert_runtime: Some(runtime_matrix_assert("qwen3:30b", "ollama")),
        },
        SmokeScenario {
            name: "runtime_model_matrix_ollama_cloud",
            child_mode: "report-runtime",
            write_file: None,
            expect_exit_ok: true,
            expect_status_line: "1 completed, 0 failed, 0 upstream exhausted, 0 unfinished",
            expect_merge_line: "completed (no changes)",
            runtime_profile: Some(crate::cleave::CleaveChildRuntimeProfile {
                model: Some("ollama-cloud:gpt-oss:120b-cloud".into()),
                ..Default::default()
            }),
            assert_runtime: Some(runtime_matrix_assert("gpt-oss:120b-cloud", "ollama-cloud")),
        },
    ];

    eprintln!(
        "omegon {} — cleave smoke test mode",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!(
        "Running {} deterministic cleave smoke scenario(s)...",
        scenarios.len()
    );

    let mut failed = 0usize;
    for scenario in &scenarios {
        eprint!("  {:<32} ", scenario.name);
        match run_scenario(cli, scenario).await {
            Ok(()) => eprintln!("✓ pass"),
            Err(e) => {
                failed += 1;
                eprintln!("✗ FAIL: {e:#}");
            }
        }
    }

    if failed > 0 {
        anyhow::bail!("cleave smoke suite failed: {failed} scenario(s) failed");
    }

    eprintln!("✓ all deterministic cleave smoke scenarios passed");
    Ok(())
}

async fn run_scenario(cli: &Cli, scenario: &SmokeScenario) -> anyhow::Result<()> {
    let temp_dir = std::env::temp_dir().join(format!(
        "omegon-cleave-smoke-{}-{}",
        std::process::id(),
        scenario.name
    ));
    let repo = temp_dir.join("repo");
    let workspace = temp_dir.join("workspace");
    std::fs::create_dir_all(&workspace)?;
    init_repo(&repo)?;
    if scenario.name == "runtime_profile_enforced" {
        let docs = repo.join("docs");
        std::fs::create_dir_all(&docs)?;
        std::fs::write(
            docs.join("runtime-preload.md"),
            "preloaded runtime context\n",
        )?;
        let plugins_dir = repo.join(".omegon").join("plugins");
        std::fs::create_dir_all(plugins_dir.join("alpha"))?;
        std::fs::write(
            plugins_dir.join("alpha").join("plugin.toml"),
            r#"
            [plugin]
            name = "Alpha Plugin"
            description = "Alpha test plugin"

            [activation]
            always = true

            [[tools]]
            name = "alpha_tool"
            description = "does alpha"
            endpoint = "http://localhost:9999/alpha"
        "#,
        )?;
        std::fs::create_dir_all(plugins_dir.join("beta"))?;
        std::fs::write(
            plugins_dir.join("beta").join("plugin.toml"),
            r#"
            [plugin]
            name = "Beta Plugin"
            description = "Beta test plugin"

            [activation]
            always = true

            [[tools]]
            name = "beta_tool"
            description = "does beta"
            endpoint = "http://localhost:9999/beta"
        "#,
        )?;
    }

    // temp_dir is PID-namespaced; best-effort cleanup on success
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    let plan_json = r#"{
  "children": [
    {
      "label": "smoke-child",
      "description": "Deterministic cleave smoke child.",
      "scope": ["README.md"],
      "depends_on": []
    }
  ],
  "rationale": "deterministic cleave smoke"
}
"#;
    let plan: cleave::CleavePlan = serde_json::from_str(plan_json)?;
    let injected_env = scenario_env(scenario);
    let mut config = cleave::orchestrator::CleaveConfig {
        agent_binary: std::env::current_exe()?,
        bridge_path: PathBuf::new(),
        node: String::new(),
        model: cli.model.clone(),
        max_parallel: 1,
        timeout_secs: 30,
        idle_timeout_secs: 10,
        max_turns: 2,
        inventory: None,
        inherited_env: vec![],
        injected_env,
        child_runtime: scenario.runtime_profile.clone().unwrap_or_default(),
        progress_sink: cleave::progress::stdout_progress_sink(),
        workflow: None,
    };
    if let Some(profile) = &scenario.runtime_profile {
        config.child_runtime = profile.clone();
    }

    let result = cleave::run_cleave(
        &plan,
        "deterministic cleave smoke",
        &repo,
        &workspace,
        &config,
        CancellationToken::new(),
        None,
    )
    .await?;

    let (completed, failed, upstream_exhausted, unfinished) =
        crate::summarize_cleave_child_statuses(&result.state.children);
    let status_line = format!(
        "{completed} completed, {failed} failed, {upstream_exhausted} upstream exhausted, {unfinished} unfinished"
    );
    if !status_line.contains(scenario.expect_status_line) {
        anyhow::bail!(
            "status summary mismatch: expected {:?}, got {:?}",
            scenario.expect_status_line,
            status_line
        );
    }

    let merge_line = crate::format_cleave_merge_result(
        result.state.children.first(),
        "smoke-child",
        &result.merge_results[0].1,
    );
    if !merge_line.contains(scenario.expect_merge_line) {
        anyhow::bail!(
            "merge line mismatch: expected {:?}, got {:?}",
            scenario.expect_merge_line,
            merge_line
        );
    }

    let terminal_ok = failed == 0 && upstream_exhausted == 0 && unfinished == 0;
    if terminal_ok != scenario.expect_exit_ok {
        anyhow::bail!(
            "terminal success mismatch: expected {}, got status summary {:?}",
            scenario.expect_exit_ok,
            status_line
        );
    }

    if let Some(assert_runtime) = &scenario.assert_runtime {
        let stdout = result.state.children[0].stdout.as_deref().unwrap_or("");
        let report: serde_json::Value = serde_json::from_str(stdout.trim())
            .with_context(|| format!("expected JSON runtime report, got: {stdout:?}"))?;
        assert_runtime(&report)?;
    }

    Ok(())
}

fn assert_runtime_profile_report(report: &serde_json::Value) -> anyhow::Result<()> {
    let tool_names = report["tool_names"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("tool_names missing"))?
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    if tool_names.contains(&"bash") {
        anyhow::bail!("bash should be disabled in runtime report: {tool_names:?}");
    }
    let plugin_names = report["plugin_names"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("plugin_names missing"))?
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    if plugin_names.iter().any(|name| *name == "Beta Plugin") {
        anyhow::bail!("disabled extension loaded: {plugin_names:?}");
    }
    if report["thinking_level"] != "high" {
        anyhow::bail!("thinking level not applied: {report}");
    }
    if report["context_class"] != "legion" && report["context_class"] != "Legion" {
        anyhow::bail!("context class not applied: {report}");
    }
    if report["model"] != "claude-sonnet-4-6" {
        anyhow::bail!("model not applied: {report}");
    }
    if report["provider"] != "anthropic" {
        anyhow::bail!("provider not inferred from model: {report}");
    }
    let requested_skills = report["requested_skill_filter"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("requested_skill_filter missing"))?
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    if requested_skills != vec!["security"] {
        anyhow::bail!("requested skill filter not applied: {requested_skills:?}");
    }
    let preloaded_files = report["preloaded_files"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("preloaded_files missing"))?;
    let preload_hit = preloaded_files.iter().any(|entry| {
        entry["path"] == "docs/runtime-preload.md"
            && entry["content"] == "preloaded runtime context\n"
    });
    if !preload_hit {
        anyhow::bail!("preloaded file not surfaced in runtime report: {report}");
    }
    Ok(())
}

fn scenario_env(scenario: &SmokeScenario) -> Vec<(String, String)> {
    let mut vars = vec![
        (
            "OMEGON_CLEAVE_SMOKE_CHILD_MODE".to_string(),
            scenario.child_mode.to_string(),
        ),
        // Suppress keychain access in smoke children — avoids macOS
        // password prompts and hangs in headless/CI contexts.
        ("OMEGON_NO_KEYRING".to_string(), "1".to_string()),
    ];
    if let Some(path) = scenario.write_file {
        vars.push((
            "OMEGON_CLEAVE_SMOKE_WRITE_FILE".to_string(),
            path.to_string(),
        ));
    }
    vars
}

fn init_repo(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    run_git(path, ["init", "-q"])?;
    run_git(path, ["config", "user.email", "smoke@example.com"])?;
    run_git(path, ["config", "user.name", "Smoke Test"])?;
    std::fs::write(path.join("README.md"), "hello smoke\n")?;
    run_git(path, ["add", "README.md"])?;
    run_git(path, ["commit", "-qm", "init"])?;
    Ok(())
}

fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) -> anyhow::Result<()> {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("failed to run git {:?}", args))?;
    if !status.success() {
        anyhow::bail!("git {:?} failed with status {status}", args);
    }
    Ok(())
}
