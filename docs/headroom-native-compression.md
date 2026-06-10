# Native Headroom-Compatible Compression

## Decision

Omegon 0.27 will implement a native Rust compression subsystem inspired by Headroom's content-routing and Compress-Cache-Retrieve (CCR) model. It will not shell out to, vendor, or depend on the Python/TypeScript Headroom packages.

## Goals

- Compress large tool outputs and context surfaces before they enter the LLM prompt.
- Preserve reversibility by storing exact originals behind stable content references.
- Keep the first implementation deterministic, local-first, and dependency-light.
- Leave room for future protocol/tool compatibility with Headroom-style `compress`, `retrieve`, and `stats` surfaces.

## Non-goals for the first 0.27 slice

- No Python dependency.
- No local HTTP proxy.
- No ML/ONNX compression path.
- No TUI rendering changes.
- No memory-crate integration while the memory surface workstream is active.

## Initial crate boundary

`core/crates/omegon-headroom` owns:

- content kind detection (`json`, `log`, `diff`, `code`, `markdown`, `plain_text`)
- deterministic compression policies
- per-kind compression summaries
- CCR references and in-memory retrieval primitives
- size/savings statistics

Omegon integration should first target host-context/read and large tool result boundaries, where today's behavior is hard truncation. The desired behavior is a compact high-signal summary plus a retrieval handle for exact original content.

## Experimental opt-in surface

Native compression remains disabled by default while Omegon dogfoods the subsystem and collects benchmark evidence. Integration points must check the runtime feature gate before modifying prompt/tool payloads.

Runtime settings carry the experimental surface:

```rust
pub struct HeadroomRuntimeConfig {
    pub enabled: bool,
    pub mode: HeadroomCompressionMode, // off | manual | on
    pub collect_metrics: bool,
    pub reversible: bool,
    pub min_bytes: usize,
    pub target_bytes: usize,
    pub local_model: Option<String>,
}
```

Defaults are conservative:

- `enabled = false`
- `mode = off`
- `collect_metrics = true`
- `reversible = true`
- `min_bytes = 50 KiB`
- `target_bytes = 16 KiB`
- `local_model = None` (deterministic-only)

The agent-facing control surface is `harness_settings` with `action = "set_headroom_compression"`:

```json
{"action":"set_headroom_compression","value":"manual"}
{"action":"set_headroom_compression","value":"on","min_bytes":102400,"target_bytes":24576}
{"action":"set_headroom_compression","value":"on","local_model":"qwen2.5-coder:7b"}
{"action":"set_headroom_compression","value":"off"}
```

Mode semantics:

- `off`: no automatic compression. Existing truncation behavior remains unchanged.
- `manual`: compression may run only for explicit tool/operator actions. This is the dogfood default when an operator opts in.
- `on`: enabled integration points may compress automatically using conservative policy.

No mode may silently download, start, or require a local semantic model. If `local_model` is configured but unavailable, the deterministic compressor remains the fallback and validation metadata should report the fallback.

## Evaluation workflow

Native compression effectiveness is measured by the `headroom-eval` binary in the `omegon-headroom` crate:

```bash
cargo run -p omegon-headroom --bin headroom-eval -- --text
cargo run -p omegon-headroom --bin headroom-eval -- --json
```

The evaluator runs canonical fixtures through the deterministic compressor and exits non-zero if any fixture violates its contract. The text output is intended for local dogfooding; JSON output is intended for CI snapshots and longitudinal benchmark comparison.

The first canonical suite covers:

- JSON arrays with sparse critical rows
- long Cargo-style failure logs
- compact grep-style output that must pass through unchanged
- small Rust source snippets that must pass through unchanged

Current baseline from the suite:

```text
Headroom evaluation: PASS
fixtures: 6
evaluated bytes: 59428 -> 6435 (89% saved)
estimated tokens (bytes_div_4/approximate): 14859 -> 1611 (89% saved)
```

The report distinguishes raw compressor output from evaluated output after protected required-fact restoration. Pass/fail savings thresholds are evaluated against the final text that would enter context, not against raw compressor stats. Reports also include fixture classes, raw missing facts, restored fact counts/bytes, and the token counter identity so benchmark consumers do not mistake `bytes_div_4` estimates for provider-tokenizer measurements.

Protected anchors are expected to be preserved by compressors where possible. Any remaining restored facts in the evaluation report are treated as design pressure on the relevant compressor, not as proof that production automatic compression is safe.

This baseline is intentionally small. It proves the harness and catches obvious regressions; it is not sufficient evidence to enable compression by default. Enabling by default requires dogfood fixtures from real Omegon sessions and stable savings/fact-retention results across tool outputs, build logs, source reads, and markdown/design surfaces.

## Validation and Token-Savings Claims

Omegon does not claim compression effectiveness by inspection. Every compressor is validated against canonical fixtures. A fixture defines input text, content kind, required facts, expected compression behavior, and a minimum savings threshold.

The validator records original/compressed bytes, estimated original/compressed tokens, byte savings percentage, token savings percentage, latency, required-fact retention, and pass/fail reasons.

The default token counter is deterministic and dependency-free: `ceil(bytes / 4)`. Provider/model-specific tokenizers may be added later as optional counters, but they are not required for CI.

Local semantic models are optional evaluators. They may score semantic adequacy or produce model-authored summaries, but deterministic fact-retention and CCR retrieval remain the correctness gate.

The first production compressor pass includes protected-anchor extraction for plain text, markdown, logs, and diffs before excerpt trimming. Protected anchors include signal words, decision lines, task checkboxes, path:line references, hash-like tokens, nonzero exit/status codes, and test-count summaries. This reduces dependence on evaluator-only required-fact restoration and is a prerequisite for enabling automatic compression beyond dogfood mode.

## Compression model strategy

Omegon should treat "compression model" as a pluggable local capability, not as a required dependency for the native subsystem.

The default 0.27 model is deterministic and rule-based:

- content detection routes text into `json`, `log`, `diff`, `code`, `markdown`, or `plain_text`
- each route uses bounded local summarizers, not inference
- outputs are stable across runs for the same input and policy
- no network access is required
- no Python, ONNX, Hugging Face, or external model runtime is required

This gives Omegon a reliable baseline that works everywhere the Rust binary runs. It also prevents context compression from becoming a startup/install blocker.

Future learned compression can be added as an optional provider behind the same trait boundary:

```rust
pub trait CompressionModel {
    fn name(&self) -> &'static str;
    fn availability(&self) -> ModelAvailability;
    fn compress(&self, input: CompressionInput) -> anyhow::Result<CompressionOutput>;
}
```

The selection policy should be:

1. use an explicitly configured local model if available and compatible with the content kind
2. otherwise fall back to the deterministic native compressor
3. never fail a tool call only because a learned compression model is missing
4. always preserve CCR retrieval for exact originals when reversible mode is enabled

Optional local models must be packaged as Omegon capabilities, not implicit runtime downloads. A future ONNX/Kompress-style backend may live behind a feature flag such as `headroom-ml`, but the base `omegon-headroom` crate must remain dependency-light and deterministic.

## Compression policy

The first policy is conservative:

- skip compression below `min_bytes`
- keep outputs readable markdown/plain text
- preserve first/last excerpts
- preserve high-signal lines containing error, warning, panic, failure, TODO/FIXME, or changed diff hunks
- include original byte counts, compressed byte counts, content hash, and retrieval id when reversible mode is enabled
- mark which compression model produced the output once model selection exists; the initial value is the deterministic native model

## Ollama coupling

Ollama should be an optional semantic compression backend, not the primary availability mechanism for native headroom compression.

Omegon already exposes local inference through Ollama-compatible tools (`ask_local_model`, `list_local_models`, `manage_ollama`) against the OpenAI-compatible API at `LOCAL_INFERENCE_URL` or `http://localhost:11434`. The headroom subsystem should reuse that capability boundary for model discovery and operator-managed installation, while keeping deterministic compression independent of a running Ollama server.

The intended layering is:

- deterministic compressor: default production path for structured/tool-output surfaces
- Ollama semantic compressor: optional path for prose-heavy markdown, conversation history, issue threads, and mixed narrative logs
- future ONNX/token-classifier compressor: optional purpose-built low-latency learned path behind a feature flag or capability package

Ollama-backed compression must be hybrid and validator-protected:

1. deterministic extraction preserves protected anchors such as paths, IDs, hashes, counts, errors, warnings, stack traces, and retrieval ids
2. the Ollama model compresses only the prose/body regions selected by policy
3. validation rejects outputs that grow, omit protected anchors, exceed latency/budget limits, or lack the CCR retrieval handle
4. failed, missing, slow, or unhealthy Ollama calls fall back to deterministic compression

Ollama model acquisition should delegate to the existing Ollama lifecycle rather than duplicating it:

- list/status: `GET /v1/models` through the local inference client
- install/update: operator-approved `ollama pull <model>` via `manage_ollama` semantics
- remove: `ollama rm <model>` when exposed
- health: small probe completion plus timeout/latency measurement

The default policy should discover and report Ollama availability but not silently download or start models. A future config may opt in to automatic startup or install for daemon deployments, but interactive sessions should present an explicit recommendation such as `manage_ollama action=pull model=<name>`.

## CCR semantics

A compressed output may contain a `HeadroomRef`:

- `id`: stable `hr:<sha256-prefix>` identifier
- `sha256`: full content hash
- `bytes`: original UTF-8 byte length
- `content_kind`: detected or supplied content kind

The original content remains retrievable by id. Future on-disk stores can use the same reference shape under `.omegon/headroom/objects/`.
