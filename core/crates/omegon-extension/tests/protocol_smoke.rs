//! SDK self-test — proves the v2 protocol wire contract.
//!
//! Spawns the bundled `test-extension` binary and exchanges
//! JSON-RPC messages with it over stdin/stdout, exactly as a real
//! omegon host would. Catches breaking changes to:
//!
//! - The `initialize` handshake shape (protocol version, capabilities)
//! - `tools/list` response schema
//! - `tools/call` request/response shape
//! - Newline-delimited JSON framing
//!
//! This test belongs in the SDK crate (not the omegon host crate)
//! because it's the SDK's job to guarantee the wire contract.
//! Downstream extensions (shuttle, scry, vox, etc.) rely on this
//! contract — if it changes silently, every extension breaks.
//!
//! Runs in CI on every push. Failure here means we broke the SDK
//! and any release that ships this state will break extensions
//! built against the new version.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Locate the test-extension binary cargo built alongside the test.
/// `CARGO_BIN_EXE_<name>` is set by cargo at test time for any
/// `[[bin]]` in the same package.
fn test_extension_binary() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_test-extension").into()
}

/// One-shot RPC harness: spawn the extension, send a request,
/// read the response, drop the child. Suitable for individual
/// protocol smoke checks where session continuity isn't needed.
fn rpc_one_shot(method: &str, params: Value) -> Value {
    let mut child = Command::new(test_extension_binary())
        // test-extension exits unless told it's running as an RPC
        // server (rather than being launched accidentally for inspection).
        .arg("--rpc")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn test-extension");

    let stdin = child.stdin.as_mut().expect("stdin");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });
    writeln!(stdin, "{request}").expect("write request");
    stdin.flush().expect("flush");

    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response");

    // Best-effort cleanup. The extension is single-threaded over
    // stdin so it should exit when we close stdin via drop(child).
    let _ = child.kill();
    let _ = child.wait();

    serde_json::from_str(&line).expect("response is valid JSON")
}

#[test]
fn initialize_returns_protocol_version_2() {
    let resp = rpc_one_shot("initialize", json!({}));
    assert_eq!(resp["jsonrpc"], "2.0");
    let result = &resp["result"];
    assert_eq!(
        result["protocol_version"], 2,
        "v2 handshake required — found {result:#?}"
    );
}

#[test]
fn initialize_reports_extension_identity() {
    let resp = rpc_one_shot("initialize", json!({}));
    let info = &resp["result"]["extension_info"];
    assert_eq!(info["name"], "test-extension");
    assert!(
        info["version"].is_string(),
        "extension version must be a string — found {info:#?}"
    );
    assert!(
        info["sdk_version"].is_string(),
        "sdk_version must be a string — found {info:#?}"
    );
}

#[test]
fn initialize_advertises_tools_capability() {
    let resp = rpc_one_shot("initialize", json!({}));
    let caps = &resp["result"]["capabilities"];
    assert_eq!(
        caps["tools"], true,
        "test-extension declares tools support — found {caps:#?}"
    );
}

#[test]
fn tools_list_returns_array() {
    let resp = rpc_one_shot("tools/list", json!({}));
    // The v2 contract: tools/list result is the array itself, not
    // wrapped in `{ tools: [...] }`. Pinning this shape so a future
    // change to wrap it would land as a clear test failure rather
    // than silently breaking every host that decodes tools/list.
    let tools = resp["result"]
        .as_array()
        .cloned()
        .expect("tools/list result must be an array");
    assert!(
        !tools.is_empty(),
        "test-extension exposes at least the echo tool"
    );
    assert!(
        tools.iter().any(|t| t["name"] == "echo"),
        "echo tool present — found {tools:#?}"
    );
}

#[test]
fn tools_call_echo_round_trips_payload() {
    let resp = rpc_one_shot(
        "tools/call",
        json!({
            "name": "echo",
            "arguments": { "message": "hello, omegon" }
        }),
    );
    // Different extensions can shape result content differently; we
    // just need to find the message somewhere in the response payload.
    let s = resp["result"].to_string();
    assert!(
        s.contains("hello, omegon"),
        "echo response must round-trip the message — got {s}"
    );
}

#[test]
fn unknown_method_returns_error_not_panic() {
    let resp = rpc_one_shot("nonexistent/method", json!({}));
    assert!(
        resp["error"].is_object(),
        "unknown method must return JSON-RPC error, not panic — got {resp:#?}"
    );
}

#[test]
fn spawn_responds_within_one_second() {
    // Cold-start budget. If the SDK starts eating seconds at boot
    // (e.g. blocking dns, blocking file IO before initialize),
    // every extension's first-request latency degrades. Catch it.
    let start = std::time::Instant::now();
    let _ = rpc_one_shot("initialize", json!({}));
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "initialize round-trip took {elapsed:?} — should be <1s on a cold spawn"
    );
}
