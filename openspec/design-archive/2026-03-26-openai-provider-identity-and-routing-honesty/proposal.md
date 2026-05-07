+++
id = "5eb2c602-9375-4728-8173-2f7d66ec8dce"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenAI provider identity and routing honesty — separate API vs ChatGPT/Codex auth, route GPT models correctly, and surface the active engine truthfully

## Intent

The Rust harness currently treats ChatGPT/Codex OAuth as if it authenticated the generic openai provider, causing the UI to advertise GPT-family models under openai:* even when only openai-codex credentials exist. At runtime, openai:* routes through OpenAIClient (API key + Chat Completions semantics) while the available credential may actually be an openai-codex JWT that only works through CodexClient. The design goal is to restore honesty at three layers: credential identity (OpenAI API vs ChatGPT/Codex OAuth), routing identity (GPT-family models choose the provider/client that can actually execute them), and operator visibility (the UI clearly shows which provider/model/credential path is active right now).

See [design doc](../../../docs/openai-provider-identity-and-routing-honesty.md).
