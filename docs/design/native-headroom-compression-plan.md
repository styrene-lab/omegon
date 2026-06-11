+++
title = "Native Headroom Compression Plan"
tags = ["headroom","compression","evaluation","dogfood"]
+++

# Native Headroom Compression Plan

---
title: Native Headroom Compression Plan
status: implementing
tags: [headroom, compression, evaluation, dogfood]
---

# Native Headroom Compression Plan

## Overview

Omegon is building a Rust-native, Headroom-inspired context compression subsystem. The current implementation is deterministic and model-free: it ships inside the `omegon-headroom` crate and does not download, vendor, or require a learned compression artifact. Learned/model-backed compression remains future provider work behind an explicit provider boundary.

Related long-form design: [[docs/headroom-native-compression|Native Headroom-Compatible Compression]].

## Current decisions

- The active provider is `native_deterministic`, built into the Omegon binary.
- Compression remains experimental and opt-in.
- Manual CCR tools exist before automatic compression expands beyond `read`.
- Automatic compression is currently gated to `headroom.mode = on`.
- The first automatic integration point is `read` because it already has file-boundary checks and truncation policy.
- Ranked/navigation tools should be capped first, evaluated second, CCR-overflow third, and generically compressed last.
- Required-fact restoration is an evaluation repair mechanism, not proof of production safety.
- Strict dogfood should ratchet toward `--max-restored-facts 0`.
- Local learned providers such as Kompressor must be optional, explicit, versioned, and validator-protected.

## Implemented

- `omegon-headroom` crate with deterministic compression primitives.
- CCR references and session in-memory retrieval.
- Bounded CCR store policy and eviction stats.
- Native provider identity metadata.
- Experimental runtime settings: `off | manual | on`.
- Manual tools: `headroom_compress`, `headroom_retrieve`, `headroom_stats`.
- Shared session CCR store plumbing for future integration points.
- Gated `read` integration.
- `headroom-eval` with canonical/adversarial/dogfood fixture support.
- `--save`, `--compare`, `--fixtures`, token-counter selection, and restoration-budget gating.
- `just headroom-dogfood` for automated local dogfood fixture generation.
- `just headroom-fixture` for turning real samples into editable fixtures.
- Result-window caps for search/memory/session-log surfaces.

## Remaining work plan

### Phase A — stabilize current foundation

- [ ] Commit current foundation slice: provider metadata, bounded CCR store, docs, strict dogfood tuning.
- [ ] Add a regression fixture directory for redacted committed fixtures if any dogfood samples can be safely checked in.
- [ ] Ensure `just headroom-dogfood` and strict `headroom-eval` are documented as the local gate.
- [ ] Keep `.tmp/headroom` outputs ignored and non-authoritative.

### Phase B — provider boundary

- [ ] Introduce `CompressionProvider` trait.
- [ ] Move deterministic implementation behind `NativeDeterministicProvider`.
- [ ] Preserve the current public behavior and provider identity.
- [ ] Extend `headroom-eval` with `--provider native_deterministic` as a no-op baseline.
- [ ] Add comparison output that can compare providers on the same fixture set.

### Phase C — broaden dogfood fixture corpus

- [ ] Add fixture-corpus manifest format for local datasets.
- [ ] Generate fixtures from real command/tool outputs, document/code corpora, and public repositories.
- [ ] Add corpus-level labels: `canonical`, `adversarial`, `dogfood`, `regression`, `corpus`.
- [ ] Track class/domain metrics separately so aggregate savings cannot hide domain regressions.
- [ ] Add automated fixture minimization/redaction guidance before any fixture is committed.

### Phase D — integrate with existing benchmark harness

- [ ] Add a benchmark task under `ai/benchmarks/tasks/` that exercises headroom read compression in a controlled repo.
- [ ] Teach `scripts/benchmark_harness.py` to pass headroom dogfood env/settings when a task opts in.
- [ ] Capture headroom eval artifacts beside benchmark result JSON.
- [ ] Add benchmark report fields for headroom savings/restoration counts.
- [ ] Keep this optional until it is cheap and deterministic enough for CI.

### Phase E — additional automatic integration points

Only after corpus evidence is strong:

- [ ] Consider CCR-overflow for `codebase_search` full result sets.
- [ ] Consider CCR-overflow for web/search extraction payloads.
- [ ] Consider markdown/design doc read surfaces.
- [ ] Do not generically compress ranked result lists unless caps and CCR-overflow are insufficient.

### Phase F — learned provider milestone

- [ ] Define model artifact lifecycle: install, update, integrity, version reporting.
- [ ] Add optional Kompressor provider behind the provider trait.
- [ ] Require deterministic protected-anchor extraction before model summarization.
- [ ] Reject model output on growth, anchor loss, timeout, or CCR failure.
- [ ] Gate with `just headroom-dogfood` + `headroom-eval --provider kompressor --max-restored-facts 0`.

## Open questions

- [assumption] The existing benchmark harness can cheaply invoke headroom dogfood/eval without making normal benchmarks too slow.
- [assumption] Public repo corpora can be cloned during local dogfood but not CI unless cached or minimized.
- [assumption] PDF/EPUB extraction should use existing readable-text conversion paths or external tools only in local corpus generation, not runtime compression.
- What fixture corpus should be committed versus generated locally?
- What provider-tokenizer should become the first non-approximate token counter?
- What is the right on-disk CCR retention/redaction policy if session-only storage becomes insufficient?

## Candidate dogfood corpus

### Runtime/tool outputs

- Cargo test logs from `omegon`, `omegon-headroom`, and one large workspace-filter test.
- Cargo check/clippy failure logs with real compiler diagnostics.
- `git show --stat --patch` across recent headroom commits.
- `git diff` for synthetic large edits containing important scattered hunks.
- `codebase_search` JSON/details output for broad queries.
- `memory_recall` and session-log outputs with bounded windows.
- `read` output for large markdown, Rust, TOML, JSON, and lock files.

### Documents

- Large markdown design docs in this repo.
- Public-domain PDF converted to text.
- Public-domain EPUB converted to text/markdown.
- API specs: OpenAPI JSON/YAML, AsyncAPI, large generated schema files.
- Changelogs/release notes with many versions and sparse breaking-change lines.

### Public repositories

Clone locally under ignored `.tmp/headroom/repos/` and generate fixtures from `git grep`, `git log`, `git diff`, and representative large files.

Suggested repos:

- `rust-lang/rust` — huge Rust codebase and diagnostics-shaped logs.
- `tokio-rs/tokio` — Rust async library with docs/tests.
- `ratatui/ratatui` — relevant TUI/docs/code patterns.
- `dioxuslabs/dioxus` — relevant future UI integration corpus.
- `kubernetes/kubernetes` — large Go/YAML/log/config corpus.
- `microsoft/vscode` — large TypeScript/JSON corpus.
- `fastapi/fastapi` — Python/docs/OpenAPI-adjacent corpus.

### Adversarial/generated

- Sparse critical JSON rows hidden in thousands of normal rows.
- Logs with many false-positive `error` strings in benign contexts.
- Diff hunks where important facts are in function names rather than failure lines.
- Markdown with decisions buried far from headings.
- JSON arrays where important facts are encoded as status/severity/exit_code rather than `error` keys.
- Small outputs that must pass through unchanged.
- Outputs just above/below threshold.
- Multibyte UTF-8 near clipping boundaries.

## Acceptance gates for dogfood expansion

- Canonical/adversarial fixtures pass.
- Dogfood corpus passes with `--max-restored-facts 0` for deterministic provider before default-on expansion.
- Any fixture with `expected_compressed=true` must show positive evaluated savings.
- No output may grow after compression.
- Per-domain class metrics must be reported; aggregate suite savings is insufficient evidence.
- Retrieval handles must resolve while stored and fail clearly after eviction.
