---
task_id: 2
label: docs
siblings: [0:enforcement, 1:tui-surfaces]
---

# Task 2: docs

## Root Directive

> Implement Anthropic subscription ToS compliance enforcement with documentation

## Mission

Write user-facing documentation about Anthropic subscription ToS compliance. Two deliverables: (1) A new markdown doc at docs/anthropic-subscription-tos.md covering: what the restriction is (exact ToS quote), which Omegon entry points are allowed vs blocked, the interactive-vs-automated distinction, what to do if you need automation (get an API key), and how this compares to other providers (GitHub Copilot has no such restriction). Include a clear table: TUI mode=allowed, --initial-prompt=allowed, --prompt/--prompt-file=blocked, --smoke=blocked, /cleave=blocked. Include the exact Anthropic ToS URL. Make it factual and non-alarmist — this is a clear boundary that Omegon enforces for the operator's protection. (2) A new Astro page at site/src/pages/docs/providers.astro covering all provider auth modes: Anthropic API key (unrestricted), Anthropic OAuth/subscription (interactive TUI only), OpenAI API key, Codex OAuth, Ollama (local, unrestricted), GitHub Models (official PAT, coming soon), Copilot seat (coming soon). For each: how to configure, what's allowed, any restrictions. Follow the structure and style of existing pages like site/src/pages/docs/quickstart.astro or site/src/pages/docs/security.astro. Files: docs/anthropic-subscription-tos.md, site/src/pages/docs/providers.astro

## Scope

- `docs/anthropic-subscription-tos.md`
- `site/src/pages/docs/providers.astro`

**Depends on:** none (independent)

## Siblings

- **enforcement**: Hard-block automated/headless entry points in main.rs when ANTHROPIC_OAUTH_TOKEN is the sole Anthropic credential (no ANTHROPIC_API_KEY present). At the top of the main() startup path, before any LLM call or TUI launch, check: if (prompt.is_some() || smoke || smoke_cleave) AND the resolved Anthropic credential is OAuth-only, exit immediately with a clear error message. Error must name the exact ToS URL (https://www.anthropic.com/legal/consumer-terms), explain why, and tell the operator what to do (use ANTHROPIC_API_KEY instead). Also update providers.rs: add a helper function `anthropic_credential_mode() -> AnthropicCredentialMode` enum (OAuthOnly, ApiKey, None) that checks ANTHROPIC_API_KEY first then ANTHROPIC_OAUTH_TOKEN, and use it to ensure headless paths never select the OAuth token even when it's present. The block must fire BEFORE any network request. Write a test in main.rs test module that mocks the env vars and verifies the block triggers. Files: core/crates/omegon/src/main.rs, core/crates/omegon/src/providers.rs
- **tui-surfaces**: Add four Anthropic subscription ToS disclosure surfaces to the TUI. All gated on `app.footer_data.is_oauth == true && no ANTHROPIC_API_KEY`. (1) /cleave guard in handle_slash_command in tui/mod.rs: when cmd == 'cleave' and subscription-only, return SlashResult::Display with message 'Cannot run /cleave with a Claude.ai subscription — Anthropic\'s ToS prohibits automated/background use. Add ANTHROPIC_API_KEY to enable parallel agent work. See: anthropic.com/legal/consumer-terms'. (2) Startup one-time banner: add a `oauth_tos_notice_shown: bool` field to App struct. On first render after startup when is_oauth is true and no API key, queue a TUI message (via the notification/message system) telling the operator they are in interactive-only mode. Only show once per session. (3) Footer badge: in footer.rs, when is_oauth is true, append '· interactive only' to the subscription label (currently shows 'subscription' at line ~574). (4) Update tutorial.rs: in the /tutorial consent text and STEPS_ORIENTATION 'Unlock Interactive Mode' step body, add a sentence noting that background tasks and /cleave require an API key, not the subscription. Files: core/crates/omegon/src/tui/mod.rs, core/crates/omegon/src/tui/footer.rs, core/crates/omegon/src/tui/tutorial.rs

## Dependency Versions

Use these exact versions — do not rely on training data for API shapes:

```toml
# site/package.json
[dependencies]
@astrojs/sitemap = "^3.3.0"
astro = "^5.7.0"
markdown-it = "^14.1.1"
[devDependencies]
gray-matter = "^4.0.3"
```



## Testing Requirements

### Test Convention

Write tests for new functions and changed behavior — co-locate as *.test.ts


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
