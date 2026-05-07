+++
id = "bd509577-6069-46a3-b6a7-932b75832f50"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Per-Provider Input Truncation — Design Skeleton

## Problem

The current input truncation in `conversation.rs::truncate_oversized_input()` uses
a fixed 100k char limit. This works for most providers (Anthropic 200k, OpenAI 200k)
but is wrong for:
- **Ollama 7B models**: 4k-8k context, 100k chars is ~25x over budget
- **Groq small models**: 8k context on some models
- **Google Gemini**: 1M context — 100k chars is overly conservative

## Existing Infrastructure

### Context window resolution chain (already implemented)
```
settings.rs:901  infer_context_window(model: &str) -> usize
  ├─ Tries route matrix lookup (data/model-registry.json)
  ├─ Fallback heuristics per model family (opus→200k, haiku→200k, gpt-5→272k)
  ├─ Ollama default: 32k
  └─ Unknown cloud: 131k (fail-closed to Squad)
```

### Context class taxonomy (already implemented)
```
settings.rs:487  ContextClass
  Squad    → 128k tokens
  Maniple  → 272k tokens
  Clan     → 400k tokens
  Legion   → 1M+ tokens
```

### Where context_window is available at runtime
```
settings.rs:312  Settings.context_window: usize  — set at startup
settings.rs:316  Settings.context_class: ContextClass  — derived from window
loop.rs:522      stream_with_retry uses settings for compaction decisions
conversation.rs:337  needs_compaction(context_window, threshold) — already parameterized
```

## Implementation Plan

### Step 1: Make truncation context-aware

Change `truncate_oversized_input()` to accept a limit parameter:

```rust
// conversation.rs
fn truncate_oversized_input(text: String, max_chars: usize) -> String {
    if text.len() <= max_chars { return text; }
    // ... same truncation logic with dynamic limit
}
```

### Step 2: Derive limit from context window

In `push_user_with_images()`, pass the context-aware limit:

```rust
// conversation.rs
pub fn push_user_with_images(&mut self, text: String, images: Vec<...>) {
    // Derive max input from context window:
    // A single message should use at most 50% of the context window
    // (leaves room for system prompt, tools, conversation history)
    let max_chars = self.max_single_message_chars();
    let text = truncate_oversized_input(text, max_chars);
    // ...
}

fn max_single_message_chars(&self) -> usize {
    // context_window is in tokens, chars ≈ tokens * 4
    // Cap a single message at 50% of the window
    let window_tokens = self.context_window.unwrap_or(131_072);
    let max_tokens = window_tokens / 2;
    max_tokens * 4  // tokens → chars
}
```

### Step 3: Thread context_window into ConversationState

Currently `ConversationState` doesn't know the context window. Two options:

**Option A (minimal):** Add `context_window: Option<usize>` field to ConversationState,
set it from Settings at construction time in setup.rs.

**Option B (cleaner):** ConversationState already takes `context_window` as a parameter
to `needs_compaction()`. Use the same pattern — caller passes the limit.

Option A is simpler for `push_user` since it's called from many sites without
a context_window parameter. The field would default to `None` (→ 100k fallback).

### Step 4: Update adapter

The adapter truncation (100k fixed) stays as a defense-in-depth layer for the
OS ARG_MAX limit. The omegon-side truncation handles per-provider limits.

### Per-Provider Limits (reference)

| Provider | Model | Context Window | Max Single Message (50%) | Max Chars |
|---|---|---|---|---|
| Anthropic | claude-sonnet-4-6 | 200k tokens | 100k tokens | ~400k chars |
| Anthropic | claude-haiku-4-5 | 200k tokens | 100k tokens | ~400k chars |
| OpenAI | gpt-5.4 | 272k tokens | 136k tokens | ~544k chars |
| Google | gemini-2.5-flash | 1M tokens | 500k tokens | ~2M chars |
| Groq | llama-3.3-70b | 128k tokens | 64k tokens | ~256k chars |
| Ollama | llama3-8b | 8k tokens | 4k tokens | ~16k chars |
| Ollama | codellama-34b | 16k tokens | 8k tokens | ~32k chars |

### Key Files
- `core/crates/omegon/src/conversation.rs:truncate_oversized_input()` — the truncation function
- `core/crates/omegon/src/conversation.rs:push_user_with_images()` — where truncation is called
- `core/crates/omegon/src/settings.rs:infer_context_window()` — provider→window resolution
- `core/crates/omegon/src/settings.rs:ContextClass` — window taxonomy
- `core/crates/omegon/src/setup.rs` — where Settings and ConversationState are constructed
