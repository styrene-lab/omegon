# Design

The shared registry and semantic menu own discovery, context and grade defaults. Keep existing provider selections and lower-cost tiers; change only the OpenAI/Codex frontier defaults to gpt-6-astra. Fable 5.1 is already the Anthropic default.

Reuse existing Responses input/tool conversion and streaming projection for direct OpenAI Astra. Tool calls require Responses; omit unsupported sampling/logprob parameters. Use model-aware effort normalization: API none/minimal becomes low, with low/medium/high/xhigh/max admitted. Native Codex preserves the exact Astra ID. Account eligibility errors remain explicit.

Published API context is 1,050,000 tokens with 128,000 output. The installed Codex catalog advertises a different route ceiling (872,000; default272,000), so do not copy the API ceiling into the subscription route. Record that evidence and keep route limits distinct. Use the existing context pricing notice for the API threshold above272,000 input.

Sources reviewed: https://developers.openai.com/api/docs/models/gpt-6-astra and https://developers.openai.com/api/docs/guides/latest-model ; https://platform.claude.com/docs/en/models/fable-5-1/overview . Local Codex model metadata was inspected without reading credentials. No aliases or endpoints are invented.

Stateless Responses continuation must retain completed output items, including encrypted reasoning and assistant phase, through the existing raw assistant field. Replay is fenced by provider and model identity; switching routes reconstructs semantic text/tool history instead of forwarding opaque foreign items. Request extras cannot attach remote conversation history. Embedded native OpenAI endpoints use the native factory only while transport, adapter and secret references retain embedded ownership; operator overrides retain manifest admission.

Async tools and mid-turn steering are outside this compatibility pass. Existing sequential tool contracts remain in use.
