# Wave 5 counted selection

## Scope

Branch: `feat/memory-wave5-token-selection`.
Implementation and tests: `272035ff`.
The shared selector owns eligibility, ordering, deduplication, pin resolution, and
counted whole-block packing. Hosted ambient and explicit memory packs use the same
managed request. The standalone provider uses the same policy through its renderer
hook. Schema remains v13.

The hosted counter is explicitly conservative UTF-8-byte accounting. Exact counters
are caller-supplied; no production model-tokenizer accuracy is claimed. Semantic
cache reuse and the remaining checkpoint/evaluation work remain open.

## Red–green evidence

The live multilingual budget test initially exceeded its conservative host allocation.
It now skips the oversized candidate and includes the smaller complete claim.
The finite-TTL provider test reproduced retained older source entries; static
providers now reuse the same replacement path as runtime feature injections.

`tests/token_selection.rs` covers complete formatting/accounting, zero budget,
Unicode, caller-supplied exact counting, distinct eligibility reasons, pin/retrieval
deduplication, superseded-pin resolution, low-signal policy, episode share limits,
and the 512/1,024/2,048 development-cap comparison.

Host tests compare standalone and hosted ambient output/reports, compare explicit
memory packs with the shared service result, verify profile/host cap intersection,
and preserve existing mutation, applicability, and TTL behavior.

The earlier render-budget fixture used an empty prompt and the old four-characters
heuristic. It now supplies a relevant task and asserts the accounted byte allocation;
separate low-signal tests establish the intentional pin-only behavior.

## Review

Same-executor review; no independent reviewer is claimed. Reviewed oversized-first
starvation, duplicate pins, inactive/inapplicable candidates, episode filler, empty
selections, finite-TTL accumulation, report truncation, and accounting labels.
The default renderer formats borrowed selected records, avoiding repeated copies of
unrelated fact metadata during packing. Reports describe selector inputs rather
than claiming a complete inventory of upstream retrieval exclusions.

## Gates

All gates passed:

- Six selector tests, including the deterministic development-cap comparison.
- Hosted/standalone ambient parity, explicit-pack parity, and profile/host cap intersection.
- Finite-TTL replacement regression and existing applicability/context retirement tests.
- `just test-crate omegon-memory` and all-features tests: 93 unit and 66 integration tests; the schema generator remains ignored.
- No-default-features memory tests: 89 unit and 59 integration tests.
- `RUST_TEST_THREADS=1 just test-commit`: 5,317 main-crate unit tests, default integration suites, and memory tests.
- All five portable memory campaigns.
- `just clippy-changed`, Pkl evaluation, named OpenSpec validation, and whitespace checks.

The counted-selection slice is accepted. No blocking finding remains in its
same-executor review. Semantic cache reuse, complete comparative evaluation, and
durable checkpoint scheduling remain open. The parent corpus is not archived.

Local transcripts under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:

- `sh_0a0fdf0c10018asWsWQ4WMw9kE.out`: multilingual host-budget behavioral red.
- `sh_0a20a8d49001tRkQJl57NsXv5e.out`: finite-TTL replacement behavioral red.
- `sh_0a20489dc001IAx5HS315NtDVt.out`: development fixtures and focused adapter/cap checks.
- `sh_0a20e26560016Zpl5GmvQizksy.out`: passing final gates and campaigns.
