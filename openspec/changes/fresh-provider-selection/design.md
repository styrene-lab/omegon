# Design

## Selection and state

Keep the existing String model contract to avoid an unrelated migration through every surface.
An empty string represents no selection at bootstrap; never canonicalize it into Anthropic.
CLI defaults to empty; shared settings load profile first and reapply a nonempty CLI override.
Interactive startup remains disconnected until selection. Noninteractive callers may resolve
an existing provider through the registry, retaining explicit selections and existing error behavior.
Profile capture omits an absent selection. Route readiness remains authoritative for rendering.

## Shared composer and connection flow

Guard semantic message submission before consuming editor state or adding conversation records.
Slash commands and shell behavior retain their existing paths. Use existing connect menu ownership
and native connection actions. Browsing or canceling a picker must not change drafts/attachments.
Hide model-derived thinking/context when disconnected in both terminal presentations.

## Zen provider

Use a distinct opencode-zen provider; retain keyed opencode-go unchanged. Reuse compatible
transport and provider admission. Curated free-model records carry context, capabilities, and
privacy descriptions; intersect those records with a bounded live GET of Zen's public catalog.
The model endpoint returns IDs only, so suffix matching or catalog presence alone is insufficient
proof of zero pricing or anonymous eligibility. Admit only reviewed zero-input/output public
models and reject absent models. Do not automatically cross from a free route to a paid route.
A short in-process discovery cache avoids duplicate lookup during selection; inference itself
rechecks live inventory. Definitive withdrawal invalidates the matching authoritative route
through a typed provider error; transient throttling leaves the selected route available to retry.
No repository content is sent while discovering models; inference begins on a subsequent submit.

Reference: OpenCode v2 source at 5e3100a46a6ffe8062aedb2a9649cc7bcc0926ad,
packages/core/src/plugin/provider/opencode.ts and console Zen handler.ts. The upstream public
credential marker is public; the server enforces allowAnonymous. Public API and data terms:
https://opencode.ai/docs/zen . Live availability changes independently of releases.

## Validation

Write failing tests before each behavior change. Use deterministic HTTP fixtures for catalog,
streaming, tool calls, throttling, and withdrawal; no paid inference. Run serialized focused
Rust tests, the omegon crate gate and changed Clippy. Extend private tmux acceptance to exercise
unconfigured entrypoints, draft submit/cancel, and selected connection flow, recording source,
binary hash, geometry, captures, request counts, and owned-process cleanup. Do not open native
GUI windows or modify operator credentials/configuration for tests.
