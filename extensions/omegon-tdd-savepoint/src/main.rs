use async_trait::async_trait;
use omegon_extension::{Error, Extension, SDK_CONTRACT_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

mod kernel;

const NAME: &str = "omegon-tdd-savepoint";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default)]
struct SavepointExtension;

#[async_trait]
impl Extension for SavepointExtension {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        VERSION
    }

    async fn handle_rpc(&self, method: &str, params: Value) -> omegon_extension::Result<Value> {
        match method {
            "initialize" => Ok(json!({
                "protocol_version": 2,
                "extension_info": {
                    "name": NAME,
                    "version": VERSION,
                    "sdk_version": SDK_CONTRACT_VERSION
                },
                "capabilities": {
                    "tools": true,
                    "widgets": false,
                    "mind": false,
                    "vox": false,
                    "resources": false,
                    "prompts": false,
                    "sampling": false,
                    "elicitation": false,
                    "streaming": false
                },
                "tools": tool_defs()
            })),
            "get_tools" | "tools/list" => Ok(Value::Array(tool_defs())),
            "bootstrap_secrets" | "bootstrap_config" => Ok(json!({"acknowledged": true})),
            "execute_tool" | "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("args")
                    .or_else(|| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                execute_tool(name, args)
            }
            _ => Err(Error::method_not_found(method)),
        }
    }
}

fn tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "tdd_savepoint_status",
            "description": "Check TDD savepoint extension readiness for a project.",
            "input_schema": schema(json!({
                "cwd": {"type": "string", "description": "Project working directory. Defaults to the current process directory."}
            }), vec![])
        }),
        json!({
            "name": "tdd_savepoint_presets",
            "description": "List built-in TDD savepoint command presets and optionally detect likely matches for the project.",
            "input_schema": schema(json!({
                "cwd": {"type": "string"},
                "detect": {"type": "boolean"}
            }), vec![])
        }),
        json!({
            "name": "tdd_savepoint_plan",
            "description": "Resolve a TDD savepoint command plan without executing or mutating state.",
            "input_schema": schema(json!({
                "cwd": {"type": "string"},
                "preset": {"type": "string"},
                "command": {"type": "array", "items": {"type": "string"}},
                "watch_paths": {"type": "array", "items": {"type": "string"}},
                "filetype": {"type": "string"},
                "timeout_secs": {"type": "number"},
                "emit_baseline": {"type": "boolean"},
                "persist_failures": {"type": "boolean"},
                "max_output_chars": {"type": "number"},
                "change": {"type": "string"},
                "scenario": {"type": "string"},
                "task": {"type": "string"}
            }), vec![])
        }),
        json!({
            "name": "tdd_savepoint_run",
            "description": "Run a resolved TDD savepoint command once and optionally record baseline/fail/red-green evidence.",
            "input_schema": schema(json!({
                "cwd": {"type": "string"},
                "preset": {"type": "string"},
                "command": {"type": "array", "items": {"type": "string"}},
                "watch_paths": {"type": "array", "items": {"type": "string"}},
                "filetype": {"type": "string"},
                "timeout_secs": {"type": "number"},
                "emit_baseline": {"type": "boolean"},
                "persist_failures": {"type": "boolean"},
                "record": {"type": "boolean"},
                "baseline": {"type": "boolean"},
                "change": {"type": "string"},
                "scenario": {"type": "string"},
                "task": {"type": "string"}
            }), vec![])
        }),
        json!({
            "name": "tdd_savepoint_current_diff_hash",
            "description": "Compute the current git worktree diff hash used for TDD stale-pass detection.",
            "input_schema": schema(json!({
                "cwd": {"type": "string", "description": "Working directory. Defaults to the current process directory."},
                "scopes": {"type": "array", "items": {"type": "string"}, "description": "Scope paths for diff hashing."}
            }), vec![])
        }),
        json!({
            "name": "tdd_savepoint_evidence",
            "description": "Read and classify recorded TDD savepoint evidence.",
            "input_schema": schema(json!({
                "cwd": {"type": "string", "description": "Working directory. Defaults to the current process directory."},
                "command_hash": {"type": "string"},
                "change": {"type": "string"},
                "scenario": {"type": "string"},
                "task": {"type": "string"},
                "current_diff_hash": {"type": "string"},
                "current": {"type": "boolean", "description": "Compute current_diff_hash from the worktree."},
                "scopes": {"type": "array", "items": {"type": "string"}}
            }), vec![])
        }),
    ]
}

fn schema(properties: Value, required: Vec<&str>) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn execute_tool(name: &str, args: Value) -> omegon_extension::Result<Value> {
    match name {
        "tdd_savepoint_status" => status(args),
        "tdd_savepoint_presets" => presets(args),
        "tdd_savepoint_plan" => plan_tool(args),
        "tdd_savepoint_run" => run_tool(args),
        "tdd_savepoint_current_diff_hash" => current_diff_hash(args),
        "tdd_savepoint_evidence" => evidence(args),
        _ => Err(Error::method_not_found(&format!("tool '{name}'"))),
    }
}

#[derive(Debug, Deserialize)]
struct StatusArgs {
    cwd: Option<PathBuf>,
}

fn status(args: Value) -> omegon_extension::Result<Value> {
    let args: StatusArgs = parse_args(args)?;
    let cwd = cwd_or_current(args.cwd)?;
    let git_available = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    let git_repository = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&cwd)
        .output()
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true"
        })
        .unwrap_or(false);
    let openspec_present = cwd.join("openspec/changes").is_dir();
    let project_config = cwd.join(".omegon/tdd-savepoint.toml");
    Ok(json!({
        "ready": git_available && git_repository,
        "cwd": cwd,
        "git_available": git_available,
        "git_repository": git_repository,
        "openspec_present": openspec_present,
        "project_config": {
            "path": project_config,
            "present": project_config.is_file()
        },
        "raw_event_dir": ".omegon/lifecycle/savepoints",
        "project_evidence_path": "openspec/changes/{change}/evidence/tdd-savepoints.jsonl",
        "tools": [
            "tdd_savepoint_status",
            "tdd_savepoint_presets",
            "tdd_savepoint_plan",
            "tdd_savepoint_run",
            "tdd_savepoint_current_diff_hash",
            "tdd_savepoint_evidence"
        ]
    }))
}

#[derive(Debug, Clone, Serialize)]
struct BuiltinPreset {
    name: &'static str,
    filetypes: &'static [&'static str],
    watch_paths: &'static [&'static str],
    command: &'static [&'static str],
    detect_files: &'static [&'static str],
    detect_dirs: &'static [&'static str],
}

const PRESETS: &[BuiltinPreset] = &[
    BuiltinPreset {
        name: "rust-cargo",
        filetypes: &["rs"],
        watch_paths: &["src", "tests"],
        command: &["cargo", "test"],
        detect_files: &["Cargo.toml"],
        detect_dirs: &[],
    },
    BuiltinPreset {
        name: "python-pytest",
        filetypes: &["py"],
        watch_paths: &["src", "tests"],
        command: &["pytest"],
        detect_files: &["pyproject.toml", "pytest.ini", "tox.ini"],
        detect_dirs: &["tests"],
    },
    BuiltinPreset {
        name: "typescript-vitest",
        filetypes: &["ts", "tsx", "js", "jsx"],
        watch_paths: &["src", "test", "tests"],
        command: &["pnpm", "vitest", "run"],
        detect_files: &["package.json", "vitest.config.ts", "vitest.config.js"],
        detect_dirs: &[],
    },
    BuiltinPreset {
        name: "javascript-npm-test",
        filetypes: &["js", "jsx", "ts", "tsx"],
        watch_paths: &["src", "test", "tests"],
        command: &["npm", "test"],
        detect_files: &["package.json"],
        detect_dirs: &["test", "tests"],
    },
    BuiltinPreset {
        name: "go-test",
        filetypes: &["go"],
        watch_paths: &["."],
        command: &["go", "test", "./..."],
        detect_files: &["go.mod"],
        detect_dirs: &[],
    },
    BuiltinPreset {
        name: "java-maven",
        filetypes: &["java"],
        watch_paths: &["src/main", "src/test"],
        command: &["mvn", "test"],
        detect_files: &["pom.xml"],
        detect_dirs: &[],
    },
    BuiltinPreset {
        name: "generic-just",
        filetypes: &[],
        watch_paths: &["."],
        command: &["just", "test"],
        detect_files: &["justfile", "Justfile"],
        detect_dirs: &[],
    },
    BuiltinPreset {
        name: "generic-make",
        filetypes: &[],
        watch_paths: &["."],
        command: &["make", "test"],
        detect_files: &["Makefile", "makefile"],
        detect_dirs: &[],
    },
];

#[derive(Debug, Deserialize)]
struct PresetsArgs {
    cwd: Option<PathBuf>,
    detect: Option<bool>,
}

fn presets(args: Value) -> omegon_extension::Result<Value> {
    let args: PresetsArgs = parse_args(args)?;
    let cwd = cwd_or_current(args.cwd)?;
    let detect = args.detect.unwrap_or(false);
    let presets: Vec<Value> = PRESETS
        .iter()
        .map(|preset| {
            let detected = detect
                && (preset.detect_files.iter().any(|f| cwd.join(f).is_file())
                    || preset.detect_dirs.iter().any(|d| cwd.join(d).is_dir()));
            json!({
                "name": preset.name,
                "filetypes": preset.filetypes,
                "watch_paths": preset.watch_paths,
                "command": preset.command,
                "detect_files": preset.detect_files,
                "detect_dirs": preset.detect_dirs,
                "detected": detected,
            })
        })
        .collect();
    Ok(json!({"presets": presets}))
}

#[derive(Debug, Deserialize)]
struct PlanArgs {
    cwd: Option<PathBuf>,
    preset: Option<String>,
    command: Option<Vec<String>>,
    #[serde(default)]
    watch_paths: Vec<PathBuf>,
    filetype: Option<String>,
    timeout_secs: Option<u64>,
    emit_baseline: Option<bool>,
    persist_failures: Option<bool>,
    max_output_chars: Option<usize>,
    change: Option<String>,
    scenario: Option<String>,
    task: Option<String>,
}

struct ResolvedPlan {
    cwd: PathBuf,
    command: kernel::TddCommand,
    watch_paths: Vec<PathBuf>,
    filetype: Option<String>,
    timeout_secs: u64,
    emit_baseline: bool,
    persist_failures: bool,
    max_output_chars: usize,
    change: Option<String>,
    scenario: Option<String>,
    task: Option<String>,
    sources: Value,
    warnings: Vec<String>,
}

fn resolve_plan(args: PlanArgs) -> omegon_extension::Result<ResolvedPlan> {
    let cwd = cwd_or_current(args.cwd)?;
    let preset = args
        .preset
        .as_deref()
        .and_then(|name| PRESETS.iter().find(|p| p.name == name));
    if args.preset.is_some() && preset.is_none() {
        return Err(Error::invalid_params("unknown preset"));
    }
    let command_from_call = args.command.is_some();
    let command_vec = match args.command {
        Some(command) => command,
        None => preset
            .map(|p| p.command.iter().map(|s| s.to_string()).collect())
            .ok_or_else(|| {
                Error::invalid_params("command is required when no preset supplies one")
            })?,
    };
    if command_vec.is_empty() || command_vec.iter().any(|s| s.trim().is_empty()) {
        return Err(Error::invalid_params(
            "command must be a non-empty argv array",
        ));
    }
    let command = kernel::TddCommand::new(command_vec)
        .map_err(|err| Error::invalid_params(err.to_string()))?;
    let watch_paths_from_call = !args.watch_paths.is_empty();
    let watch_paths = if watch_paths_from_call {
        args.watch_paths
    } else {
        preset
            .map(|p| p.watch_paths.iter().map(PathBuf::from).collect())
            .unwrap_or_default()
    };
    let filetype_from_call = args.filetype.is_some();
    let filetype = args
        .filetype
        .or_else(|| preset.and_then(|p| p.filetypes.first().map(|s| s.to_string())));
    let timeout_secs = args.timeout_secs.unwrap_or(60);
    let emit_baseline = args.emit_baseline.unwrap_or(false);
    let persist_failures = args.persist_failures.unwrap_or(false);
    let max_output_chars = args.max_output_chars.unwrap_or(8192);
    let sources = json!({
        "command": if command_from_call { "per-call" } else { "preset" },
        "watch_paths": if watch_paths_from_call { "per-call" } else if preset.is_some() { "preset" } else { "default" },
        "filetype": if filetype_from_call { "per-call" } else if filetype.is_some() && preset.is_some() { "preset" } else { "default" },
        "timeout_secs": if args.timeout_secs.is_some() { "per-call" } else { "default" },
        "emit_baseline": if args.emit_baseline.is_some() { "per-call" } else { "default" },
        "persist_failures": if args.persist_failures.is_some() { "per-call" } else { "default" },
        "max_output_chars": if args.max_output_chars.is_some() { "per-call" } else { "default" },
    });
    Ok(ResolvedPlan {
        cwd,
        command,
        watch_paths,
        filetype,
        timeout_secs,
        emit_baseline,
        persist_failures,
        max_output_chars,
        change: args.change,
        scenario: args.scenario,
        task: args.task,
        sources,
        warnings: Vec::new(),
    })
}

fn plan_json(plan: &ResolvedPlan) -> Value {
    json!({
        "resolved": {
            "cwd": plan.cwd,
            "command": plan.command.argv,
            "command_hash": plan.command.hash,
            "watch_paths": plan.watch_paths,
            "filetype": plan.filetype,
            "timeout_secs": plan.timeout_secs,
            "emit_baseline": plan.emit_baseline,
            "persist_failures": plan.persist_failures,
            "max_output_chars": plan.max_output_chars,
            "change": plan.change,
            "scenario": plan.scenario,
            "task": plan.task,
        },
        "sources": plan.sources,
        "warnings": plan.warnings,
    })
}

fn plan_tool(args: Value) -> omegon_extension::Result<Value> {
    let args: PlanArgs = parse_args(args)?;
    let plan = resolve_plan(args)?;
    Ok(plan_json(&plan))
}

#[derive(Debug, Deserialize)]
struct RunArgs {
    #[serde(flatten)]
    plan: PlanArgs,
    record: Option<bool>,
    baseline: Option<bool>,
}

fn run_tool(args: Value) -> omegon_extension::Result<Value> {
    let args: RunArgs = parse_args(args)?;
    let record = args.record.unwrap_or(false);
    let baseline = args.baseline.unwrap_or(false);
    let plan = resolve_plan(args.plan)?;
    let before = kernel::capture_git_identity(&plan.cwd, &plan.watch_paths);
    let prior_query = kernel::EvidenceQuery {
        command_hash: Some(plan.command.hash.clone()),
        change: plan.change.clone(),
        scenario: plan.scenario.clone(),
        task: plan.task.clone(),
        current_diff_hash: None,
    };
    let prior_events = kernel::read_events(&plan.cwd, &prior_query)
        .map_err(|err| Error::internal_error(err.to_string()))?;
    let outcome = kernel::run_command_with_timeout(
        &plan.cwd,
        &plan.command,
        Some(Duration::from_secs(plan.timeout_secs)),
    )
    .map_err(|err| Error::internal_error(err.to_string()))?;
    let after = kernel::capture_git_identity(&plan.cwd, &plan.watch_paths);
    let mut recorded_event: Option<kernel::SavepointEvent> = None;
    if record || baseline || (plan.persist_failures && outcome.state == kernel::TddState::Failing) {
        let prior_red = prior_events.iter().any(|e| {
            (e.transition == "baseline" || e.transition == "fail") && e.current_exit != Some(0)
        });
        let transition = if baseline || plan.emit_baseline {
            "baseline"
        } else if outcome.state == kernel::TddState::Passing && prior_red {
            "failing_to_passing"
        } else if outcome.state == kernel::TddState::Failing && plan.persist_failures {
            "fail"
        } else {
            "baseline"
        };
        let event_id = match transition {
            "baseline" => "baseline".to_string(),
            "fail" => format!("fail-{}", Uuid::new_v4()),
            _ => format!("redgreen-{}", Uuid::new_v4()),
        };
        let event = kernel::SavepointEvent {
            kind: "tdd_savepoint".to_string(),
            event_id,
            transition: transition.to_string(),
            command: plan.command.argv.clone(),
            command_hash: plan.command.hash.clone(),
            previous_exit: prior_events.last().and_then(|e| e.current_exit),
            current_exit: outcome.exit_code,
            watched_paths: plan.watch_paths.clone(),
            branch: after.branch.clone().or(before.branch.clone()),
            head_before: before.head.clone(),
            head_after: after.head.clone(),
            worktree_diff_hash_before: before.diff_hash.clone(),
            worktree_diff_hash_after: after.diff_hash.clone(),
            dirty_before: before.dirty,
            dirty_after: after.dirty,
            commit: None,
            change: plan.change.clone(),
            scenario: plan.scenario.clone(),
            task: plan.task.clone(),
            created_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        };
        kernel::append_event(&plan.cwd, &event)
            .map_err(|err| Error::internal_error(err.to_string()))?;
        recorded_event = Some(event);
    }
    let query = kernel::EvidenceQuery {
        command_hash: Some(plan.command.hash.clone()),
        change: plan.change.clone(),
        scenario: plan.scenario.clone(),
        task: plan.task.clone(),
        current_diff_hash: Some(after.diff_hash),
    };
    let events = kernel::read_events(&plan.cwd, &query)
        .map_err(|err| Error::internal_error(err.to_string()))?;
    let status = kernel::classify_evidence(&events, &query);
    Ok(
        json!({"planned": plan_json(&plan), "outcome": outcome, "event": recorded_event, "evidence_status": status, "evidence_status_label": status.as_str()}),
    )
}

#[derive(Debug, Deserialize)]
struct CurrentDiffHashArgs {
    cwd: Option<PathBuf>,
    #[serde(default)]
    scopes: Vec<PathBuf>,
}

fn current_diff_hash(args: Value) -> omegon_extension::Result<Value> {
    let args: CurrentDiffHashArgs = parse_args(args)?;
    let cwd = cwd_or_current(args.cwd)?;
    Ok(json!({"diff_hash": kernel::current_diff_hash(&cwd, &args.scopes)}))
}

#[derive(Debug, Deserialize)]
struct EvidenceArgs {
    cwd: Option<PathBuf>,
    command_hash: Option<String>,
    change: Option<String>,
    scenario: Option<String>,
    task: Option<String>,
    current_diff_hash: Option<String>,
    current: Option<bool>,
    #[serde(default)]
    scopes: Vec<PathBuf>,
}

fn evidence(args: Value) -> omegon_extension::Result<Value> {
    let args: EvidenceArgs = parse_args(args)?;
    let cwd = cwd_or_current(args.cwd)?;
    let current_diff_hash = if args.current.unwrap_or(false) {
        Some(kernel::current_diff_hash(&cwd, &args.scopes))
    } else {
        args.current_diff_hash
    };
    let query = kernel::EvidenceQuery {
        command_hash: args.command_hash,
        change: args.change,
        scenario: args.scenario,
        task: args.task,
        current_diff_hash,
    };
    let events = kernel::read_events(&cwd, &query)
        .map_err(|err| Error::internal_error(format!("failed to read TDD evidence: {err}")))?;
    let status = kernel::classify_evidence(&events, &query);
    Ok(json!({"status": status, "status_label": status.as_str(), "events": events}))
}

fn parse_args<T: for<'de> Deserialize<'de>>(args: Value) -> omegon_extension::Result<T> {
    serde_json::from_value(args).map_err(|err| Error::invalid_params(err.to_string()))
}

fn cwd_or_current(cwd: Option<PathBuf>) -> omegon_extension::Result<PathBuf> {
    match cwd {
        Some(path) => constrain_existing_dir(&path),
        None => std::env::current_dir().map_err(|err| Error::internal_error(err.to_string())),
    }
}

fn constrain_existing_dir(path: &Path) -> omegon_extension::Result<PathBuf> {
    let canonical = path.canonicalize().map_err(|err| {
        Error::invalid_params(format!("cwd must be an existing directory: {err}"))
    })?;
    if !canonical.is_dir() {
        return Err(Error::invalid_params("cwd must be a directory"));
    }
    Ok(canonical)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|arg| arg == "--rpc") {
        eprintln!("omegon-tdd-savepoint must be launched by Omegon with --rpc");
        std::process::exit(1);
    }

    if let Err(err) = omegon_extension::serve_v2(SavepointExtension).await {
        eprintln!("omegon-tdd-savepoint RPC server failed: {err}");
        std::process::exit(1);
    }
}
