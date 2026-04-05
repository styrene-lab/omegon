---
task_id: 1
label: tui-surfaces
siblings: [0:enforcement, 2:docs]
---

# Task 1: tui-surfaces

## Root Directive

> Implement Anthropic subscription ToS compliance enforcement with documentation

## Mission

Add four Anthropic subscription ToS disclosure surfaces to the TUI. All gated on `app.footer_data.is_oauth == true && no ANTHROPIC_API_KEY`. (1) /cleave guard in handle_slash_command in tui/mod.rs: when cmd == 'cleave' and subscription-only, return SlashResult::Display with message 'Cannot run /cleave with a Claude.ai subscription — Anthropic\'s ToS prohibits automated/background use. Add ANTHROPIC_API_KEY to enable parallel agent work. See: anthropic.com/legal/consumer-terms'. (2) Startup one-time banner: add a `oauth_tos_notice_shown: bool` field to App struct. On first render after startup when is_oauth is true and no API key, queue a TUI message (via the notification/message system) telling the operator they are in interactive-only mode. Only show once per session. (3) Footer badge: in footer.rs, when is_oauth is true, append '· interactive only' to the subscription label (currently shows 'subscription' at line ~574). (4) Update tutorial.rs: in the /tutorial consent text and STEPS_ORIENTATION 'Unlock Interactive Mode' step body, add a sentence noting that background tasks and /cleave require an API key, not the subscription. Files: core/crates/omegon/src/tui/mod.rs, core/crates/omegon/src/tui/footer.rs, core/crates/omegon/src/tui/tutorial.rs

## Scope

- `core/crates/omegon/src/tui/mod.rs`
- `core/crates/omegon/src/tui/footer.rs`
- `core/crates/omegon/src/tui/tutorial.rs`

**Depends on:** none (independent)

## Siblings

- **enforcement**: Hard-block automated/headless entry points in main.rs when ANTHROPIC_OAUTH_TOKEN is the sole Anthropic credential (no ANTHROPIC_API_KEY present). At the top of the main() startup path, before any LLM call or TUI launch, check: if (prompt.is_some() || smoke || smoke_cleave) AND the resolved Anthropic credential is OAuth-only, exit immediately with a clear error message. Error must name the exact ToS URL (https://www.anthropic.com/legal/consumer-terms), explain why, and tell the operator what to do (use ANTHROPIC_API_KEY instead). Also update providers.rs: add a helper function `anthropic_credential_mode() -> AnthropicCredentialMode` enum (OAuthOnly, ApiKey, None) that checks ANTHROPIC_API_KEY first then ANTHROPIC_OAUTH_TOKEN, and use it to ensure headless paths never select the OAuth token even when it's present. The block must fire BEFORE any network request. Write a test in main.rs test module that mocks the env vars and verifies the block triggers. Files: core/crates/omegon/src/main.rs, core/crates/omegon/src/providers.rs
- **docs**: Write user-facing documentation about Anthropic subscription ToS compliance. Two deliverables: (1) A new markdown doc at docs/anthropic-subscription-tos.md covering: what the restriction is (exact ToS quote), which Omegon entry points are allowed vs blocked, the interactive-vs-automated distinction, what to do if you need automation (get an API key), and how this compares to other providers (GitHub Copilot has no such restriction). Include a clear table: TUI mode=allowed, --initial-prompt=allowed, --prompt/--prompt-file=blocked, --smoke=blocked, /cleave=blocked. Include the exact Anthropic ToS URL. Make it factual and non-alarmist — this is a clear boundary that Omegon enforces for the operator's protection. (2) A new Astro page at site/src/pages/docs/providers.astro covering all provider auth modes: Anthropic API key (unrestricted), Anthropic OAuth/subscription (interactive TUI only), OpenAI API key, Codex OAuth, Ollama (local, unrestricted), GitHub Models (official PAT, coming soon), Copilot seat (coming soon). For each: how to configure, what's allowed, any restrictions. Follow the structure and style of existing pages like site/src/pages/docs/quickstart.astro or site/src/pages/docs/security.astro. Files: docs/anthropic-subscription-tos.md, site/src/pages/docs/providers.astro

## Dependency Versions

Use these exact versions — do not rely on training data for API shapes:

```toml
# core/crates/omegon/Cargo.toml
[dependencies]
omegon-extension = { path = "../omegon-extension" }
omegon-traits = { path = "../omegon-traits" }
omegon-git = { path = "../omegon-git" }
omegon-memory = { path = "../omegon-memory" }
omegon-codescan = { path = "../omegon-codescan" }
omegon-secrets = { path = "../omegon-secrets" }
opsx-core = { path = "../opsx-core" }
tokio = { workspace = true }
serde = { workspace = true }
toml = "0.8"
serde_json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
async-trait = { workspace = true }
clap = { workspace = true }
rusqlite = { workspace = true }
tokio-util = { workspace = true }
indexmap = { workspace = true }
dirs = "6.0.0"
unicode-truncate = "2.0"
chrono = "0.4"
libc = "0.2"
regex-lite = "0.1"
ratatui = "0.30.0"
syntect = { version = "5", default-features = false, features = ["default-syntaxes", "default-themes", "regex-onig"] }
tui-syntax-highlight = "0.2"
tachyonfx = { version = "0.25.0", features = ["sendable"] }
crossterm = "0.29.0"
reqwest = { version = "0.13.2", features = ["json", "stream"] }
tokio-stream = "0.1.18"
sha2 = "0.10.9"
secrecy = "0.10"
sysinfo = "0.33"
getrandom = "0.4.2"
open = "5.3.3"
tracing-appender = "0.2.4"
unicode-width = "0.2.2"
ratatui-image = { version = "10.0.6", default-features = false, features = ["crossterm", "image-defaults"] }
image = { version = "0.25.10", default-features = false, features = ["png", "jpeg", "gif", "webp"] }
axum = { version = "0.8.8", features = ["ws", "macros"] }
tower-http = { version = "0.6.8", features = ["cors"] }
futures-util = "0.3.32"
base64 = "0.22"
hmac = "0.12"
ansi-to-tui = "8.0"
tui-tree-widget = "0.24"
ratatui-toaster = "0.1"
ratatui-textarea = { version = "0.8", features = ["crossterm"] }
tui-popup = "0.7"
hyperrat = "0.1"
rmcp = { version = "1.2", features = ["transport-child-process", "client", "transport-streamable-http-client-reqwest", "auth"], default-features = false }
tar = "0.4"
flate2 = "1.0"
sigstore = { version = "0.13.0", default-features = false, features = ["cosign", "rustls-tls"] }
x509-parser = "0.17"
rpassword = "7"

[dev-dependencies]
insta = "1.46"
tempfile = "3.27.0"

```

## Test Convention

Follow this pattern from an existing test in the same crate:

```rust
// From bridge.rs
    #[test]
    fn llm_message_user_round_trip() {
        let msg = LlmMessage::User {
            content: "hello".into(),
            images: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"user"#));
        let parsed: LlmMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LlmMessage::User { content, .. } => assert_eq!(content, "hello"),
            _ => panic!("wrong variant"),
        }
    }
```



## Testing Requirements

### Test Convention

Write tests as #[test] functions in the same file or a tests submodule

Example from codebase:

```rust
// From bridge.rs
    #[test]
    fn llm_message_user_round_trip() {
        let msg = LlmMessage::User {
            content: "hello".into(),
            images: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"user"#));
        let parsed: LlmMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LlmMessage::User { content, .. } => assert_eq!(content, "hello"),
            _ => panic!("wrong variant"),
        }
    }
```


## Contract

1. Only work on files within your scope
2. Follow the Testing Requirements section above
3. If the task is too complex, set status to NEEDS_DECOMPOSITION

## Finalization (REQUIRED before completion)

You MUST complete these steps before finishing:

1. Run all guardrail checks listed above and fix failures
2. Commit your in-scope work with a clean git state when you are done
3. Commit with a clear message: `git commit -m "feat(<label>): <summary>"`
4. Verify clean state: `git status` should show nothing to commit

Do NOT edit `.cleave-prompt.md` or any task/result metadata files. Those are orchestrator-owned and may be ignored by git.
Return your completion summary in your normal final response instead of modifying the prompt file.

> ⚠️ Uncommitted work will be lost. The orchestrator merges from your branch's commits.

## Result

**Status:** PENDING

**Summary:**

**Artifacts:**

**Decisions Made:**

**Assumptions:**
