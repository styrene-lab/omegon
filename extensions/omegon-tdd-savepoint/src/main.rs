use async_trait::async_trait;
use omegon_extension::{Error, Extension, SDK_CONTRACT_VERSION};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

mod kernel;

const NAME: &str = "omegon-tdd-savepoint";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default)]
struct SavepointExtension;

#[async_trait]
impl Extension for SavepointExtension {
    fn name(&self) -> &str { NAME }
    fn version(&self) -> &str { VERSION }

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
        })
    ]
}

fn schema(properties: Value, required: Vec<&str>) -> Value {
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

fn execute_tool(name: &str, args: Value) -> omegon_extension::Result<Value> {
    match name {
        "tdd_savepoint_current_diff_hash" => current_diff_hash(args),
        "tdd_savepoint_evidence" => evidence(args),
        _ => Err(Error::method_not_found(&format!("tool '{name}'"))),
    }
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
        Some(path) => Ok(path),
        None => std::env::current_dir().map_err(|err| Error::internal_error(err.to_string())),
    }
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
