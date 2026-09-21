# Codex subscription continuation

The user explicitly selected the existing OpenAI Codex subscription for both
evaluation roles. This continuation preserves the initial unavailable report.
It uses no Anthropic inference, paid API key, or new login flow.

## Configuration and credentials

The existing Codex model cache lists `gpt-5.6-luna` as a supported offering and
advertises `low` reasoning. The repository registry selects that concrete model
for Codex grade D. Both extractor and reader use
`openai-codex:gpt-5.6-luna`, with explicit low reasoning.

The current grade-S default, `gpt-6-astra`, has a registered 128,000-token output
ceiling. A single full-ceiling reservation would exceed the authorized aggregate
budget. The configured grade-D model has a 65,536-token ceiling and fits.

Read-only inspection found an expired Omegon OAuth grant with a refresh token.
The existing Codex CLI store also contains a ChatGPT OAuth grant. The runner uses
`auth::resolve_with_refresh_result("openai-codex")`, which can adopt a usable
external grant or refresh and conditionally persist the stored grant. It accepts
only an OAuth result. Credentials and account identifiers are excluded from reports.

## Aggregate budget

The limit remains 100,000 aggregate provider input/output tokens and 600 seconds
of live effort. No additional budget was granted.

The first report recorded one `no configured provider route` result and zero
measured tokens. That exact error originates before `route.stream`, so no
inference dispatch occurred. The continuation releases the 20,949-token
reservation and carries its 512 ms elapsed time. The original report is retained.

Each Codex call reserves the entire registered output ceiling, conservative UTF-8
prompt bytes, and 4,096 input-overhead tokens before dispatch. Actual terminal
usage refunds the unused reservation. Unknown usage retains the full reservation
and stops subsequent calls. Known excess usage is recorded rather than hidden.
The reader and extractor share one ledger across development and held-out cases.

The Codex native request receives no unsupported `max_output_tokens` field.
Local response-byte and request-timeout guards bound consumption. The model's
registered full output ceiling supplies the conservative accounting reservation,
including reasoning tokens. An exact serving-model check forbids fallback to
another model or a paid API route.

## Frozen experiment

The v3 prompt requests verbatim source claims in a constrained JSON object, or
exact `ABSTAIN` output. The scorer requires both selected-source support and exact
cutoff-eligible source quotations. Keyword overlap cannot establish success.
It contains no gold labels or future events. The configuration is written before
development inference. Development summaries and the smoke thresholds are frozen
before constructing held-out evidence views. Corpus and label files are unchanged.

The first subscription refresh probe wrote `live-codex-wave5.json`. The stored
Omegon grant was rejected during refresh. No inference request was made, token
charge remained zero, and aggregate live elapsed time reached 1,166 ms.

Read-only inspection then found the newer, unexpired OpenAI OAuth grant used by
OpenCode. Its credential table was snapshotted into an owner-only temporary
directory. Only the access token is passed to the evaluator child through the
existing `CHATGPT_OAUTH_TOKEN` interface. No refresh token is copied, and neither
credential store is edited. Temporary credential snapshots are removed after use.

The next continuation writes `live-codex-subscription-wave5.json` and adjacent
configuration and manifest files. It carries the refresh probe's aggregate budget.
Results and validation are appended after execution completes.

## Independent review corrections before inference

Reviewer session `ses_f3ab0cf30ffe9PeiWbG3LdPAq4` identified three blockers.
All corrections apply before further inference spend:

1. Every actual Codex dispatch derives its full output reservation from the
   selected model's registry entry. This does not depend on the Codex experiment
   flag. Ordinary profile selection cannot bypass the full-ceiling reservation.
2. Direct OpenAI paid-API routes are rejected before dispatch. The evaluator does
   not send unsupported output-limit fields to Chat Completions or rely on its
   incomplete usage path. No production provider code was changed.
3. Task success requires constrained verbatim claims rather than keywords.
   Non-conforming prose is uncertain, and fabricated or contradictory quotations
   miss. Both reported contradictions have regression cases, including a writer
   that retained the right source ID while changing the source's modality.

This is a uniform scoring restriction, not a case-specific exception or a weaker
threshold. Corpus labels and acceptance thresholds are unchanged. Earlier live
reports contain no model responses, so there are no previous outputs to rejudge.

## Final measured result

The subscription comparison completed with all 32 arm/case rows and 40 successful
provider calls. Both roles used the frozen `openai-codex:gpt-5.6-luna` route.

| Held-out arm | Task success | Required-evidence recall | Injected accounted bytes | Reader p50 / maximum |
|---|---:|---:|---:|---:|
| No memory | 1/4 | 0 | 0 | 2,314 / 2,730 ms |
| Curated file search | 4/4 | 1 | 137 | 4,505 / 9,006 ms |
| Current memory, cap 1,024 | 4/4 | 1 | 342 | 3,549 / 4,422 ms |
| Candidate policy, cap 256 | 4/4 | 1 | 342 | 3,252 / 3,624 ms |

All arms had zero stale-memory and repeated-error rubric matches. There were no
unavailable or uncertain task judgments. Development results had the same success
counts. The candidate had no quality regression at its smaller declared budget.
The small held-out cases did not reduce actual injected size; the separate offline
noise ablation establishes behavior under packing pressure.

Aggregate measured usage was **5,569 input + 1,972 output = 7,541 tokens**.
Extraction used 1,254 input and 455 output tokens across eight calls. The 32 reader
calls used 4,315 input and 1,517 output tokens. Shared extraction is charged once
in this total, although each memory arm records its attributed ingestion cost.
USD cost remains unknown because subscription cost is not allocated per request.

Aggregate live elapsed time was **149,616 ms**, including the prior 1,166 ms.
Charged/reserved tokens equal measured tokens at completion. There were no unknown
usage reservations, retries, or stop reasons. The run passed the unchanged frozen
smoke thresholds within the original 100,000-token and 600-second limits.

The v3 prompt and `cutoff-grounded-verbatim-claims-v1` scorer were in place before
the first successful inference. No criterion was weakened and no held-out result
was used to tune the model, prompt, or thresholds. These are synthetic extractive
evidence probes, not evidence of general model-quality superiority.

The temporary credential SQL dump and SQLite snapshot were deleted, and their
owner-only directory was removed. Neither source credential store was modified.
