//! Headless smoke tests — validate operator features work end-to-end.
//!
//! Runs a scripted sequence of prompts through the agent loop and validates
//! that tools execute correctly, responses arrive, and the system doesn't panic.
//! Designed to run in CI or before release to catch regressions that unit tests miss.
//!
//! Usage: `omegon --smoke`
//!
//! Requires LLM auth (any provider) or local inference (Ollama).

use crate::bridge::LlmBridge;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A single smoke test case.
struct SmokeTest {
    name: &'static str,
    prompt: &'static str,
    /// Validation: check that the response contains this substring.
    expect_contains: Option<&'static str>,
    /// Validation: check that at least one tool was called.
    expect_tool_use: bool,
    /// Maximum turns to allow.
    max_turns: u32,
}

/// Run all smoke tests. Returns exit code 0 on success, 1 on failure.
pub async fn run(bridge: Arc<RwLock<Box<dyn LlmBridge>>>) -> i32 {
    let tests = vec![
        SmokeTest {
            name: "basic_response",
            prompt: "Reply with exactly the word 'pong' and nothing else.",
            expect_contains: Some("pong"),
            expect_tool_use: false,
            max_turns: 2,
        },
        SmokeTest {
            name: "tool_use_bash",
            prompt: "Run `echo hello_smoke_test` in bash and show me the output.",
            expect_contains: Some("hello_smoke_test"),
            expect_tool_use: true,
            max_turns: 3,
        },
        SmokeTest {
            name: "tool_use_read",
            prompt: "Read the file Cargo.toml and tell me the package name.",
            expect_contains: Some("omegon"),
            expect_tool_use: true,
            max_turns: 3,
        },
        SmokeTest {
            name: "multi_turn_reasoning",
            prompt: "What is 7 * 8? Reply with just the number.",
            expect_contains: Some("56"),
            expect_tool_use: false,
            max_turns: 2,
        },
    ];

    let total = tests.len();
    let mut passed = 0;
    let mut failed = 0;

    eprintln!("\n╭─── Omegon Smoke Tests ───╮");
    eprintln!("│ Running {} tests...       │", total);
    eprintln!("╰──────────────────────────╯\n");

    for test in &tests {
        eprint!("  {:<30} ", test.name);
        match run_single(test, &bridge).await {
            Ok(()) => {
                eprintln!("✓ pass");
                passed += 1;
            }
            Err(e) => {
                eprintln!("✗ FAIL: {e}");
                failed += 1;
            }
        }
    }

    eprintln!("\n  ─────────────────────────────");
    eprintln!("  {} passed, {} failed, {} total", passed, failed, total);

    if failed > 0 {
        eprintln!("\n  ✗ SMOKE TESTS FAILED\n");
        1
    } else {
        eprintln!("\n  ✓ All smoke tests passed\n");
        0
    }
}

/// Run a single smoke test — sends prompt, collects events, validates.
async fn run_single(
    test: &SmokeTest,
    bridge: &Arc<RwLock<Box<dyn LlmBridge>>>,
) -> anyhow::Result<()> {
    use crate::bridge::{LlmMessage, StreamOptions};

    let messages = vec![LlmMessage::User {
        content: test.prompt.into(),
        images: Vec::new(),
    }];

    let tools = Vec::new();
    let opts = StreamOptions {
        model: None,
        reasoning: None,
        extended_context: false,
        ..Default::default()
    };

    let bridge_guard = bridge.read().await;
    let mut event_rx = bridge_guard
        .stream(
            "You are a helpful assistant. Be concise.",
            &messages,
            &tools,
            &opts,
        )
        .await?;
    drop(bridge_guard);

    let mut response_text = String::new();
    let mut tool_used = false;
    let timeout = tokio::time::Duration::from_secs(60);

    loop {
        match tokio::time::timeout(timeout, event_rx.recv()).await {
            Ok(Some(event)) => {
                use crate::bridge::LlmEvent;
                match event {
                    LlmEvent::TextDelta { delta } => response_text.push_str(&delta),
                    LlmEvent::ToolCallEnd { .. } => tool_used = true,
                    LlmEvent::Done { message, .. } => {
                        // Extract text from the done message if we missed deltas
                        if response_text.is_empty() {
                            if let Some(text) = message
                                .get("content")
                                .and_then(|c| c.as_array())
                                .and_then(|arr| {
                                    arr.iter().find(|b| {
                                        b.get("type").and_then(|t| t.as_str()) == Some("text")
                                    })
                                })
                                .and_then(|b| b.get("text"))
                                .and_then(|t| t.as_str())
                            {
                                response_text.push_str(text);
                            }
                        }
                        break;
                    }
                    LlmEvent::Error { message } => anyhow::bail!("LLM error: {message}"),
                    _ => {}
                }
            }
            Ok(None) => break,
            Err(_) => anyhow::bail!("timeout after {timeout:?}"),
        }
    }

    // Validate
    if let Some(expected) = test.expect_contains {
        let lower = response_text.to_lowercase();
        if !lower.contains(&expected.to_lowercase()) {
            anyhow::bail!(
                "expected response to contain '{}', got: {}",
                expected,
                if response_text.len() > 100 {
                    format!("{}...", &response_text[..100])
                } else {
                    response_text
                }
            );
        }
    }

    if test.expect_tool_use && !tool_used {
        anyhow::bail!("expected tool use but none occurred");
    }

    Ok(())
}
