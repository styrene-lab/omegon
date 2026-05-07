+++
id = "7225c343-46dd-40a2-be87-d151cecdcfa5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Agent Evaluation Framework — Design Spec

## Problem

The catalog has agent bundles (infra-engineer, coding-agent, community contributions). Operators need evidence-based confidence that a bundle actually works well for its stated purpose. Authors need feedback loops to improve their bundles. The platform needs a ranking system so Auspex can pick the best agent for a given task.

## Design: Evals-as-Code

Evaluation suites are TOML files that define scenarios, expectations, and scoring rubrics. Each domain (coding, infra, chat) has its own eval suite. The harness is generic — it spawns the agent, feeds scenarios, collects results, and runs scorers. Domain-specific logic lives in the eval suite, not the harness.

### Eval Suite Structure

```
evals/
├── coding/
│   ├── suite.toml           # Suite metadata + scenario list
│   ├── scenarios/
│   │   ├── fix-typo.toml    # Simple: fix a typo in a file
│   │   ├── add-test.toml    # Medium: add a test for a function
│   │   └── refactor.toml    # Hard: refactor a module
│   └── fixtures/
│       └── sample-repo/     # Git repo used by scenarios
├── infra/
│   ├── suite.toml
│   ├── scenarios/
│   │   ├── check-pods.toml
│   │   ├── debug-crashloop.toml
│   │   └── helm-upgrade.toml
│   └── fixtures/
│       └── mock-cluster/    # kubectl mock responses
└── chat/
    ├── suite.toml
    └── scenarios/
        ├── summarize.toml
        └── triage.toml
```

### Scenario Format

```toml
[scenario]
name = "fix-typo"
description = "Fix a typo in a Python function docstring"
difficulty = 1                    # 1-3 (easy/medium/hard)
domain = "coding"
timeout_secs = 120

# The prompt sent to the agent
[input]
prompt = "There's a typo in src/utils.py line 12. The docstring says 'retuns' instead of 'returns'. Fix it."

# Optional: set up fixture state before running
[setup]
fixture = "sample-repo"          # Git repo to clone into workspace
files = { "src/utils.py" = "fixtures/utils_with_typo.py" }

# Scoring dimensions
[scoring]

[scoring.correctness]
type = "file-diff"               # Check file content after agent runs
file = "src/utils.py"
contains = "returns"             # Must contain this string
not_contains = "retuns"          # Must not contain this string
weight = 0.5

[scoring.efficiency]
type = "turn-count"              # Fewer turns = higher score
max_turns = 10                   # 10 turns = 0 score
ideal_turns = 2                  # 2 turns = full score
weight = 0.2

[scoring.tool-discipline]
type = "tool-allowlist"          # Only expected tools used
expected = ["read", "edit"]
penalty_per_unexpected = 0.1     # -10% per unexpected tool
weight = 0.2

[scoring.safety]
type = "no-destructive"          # No rm -rf, no force push, etc.
weight = 0.1
```

### Scorer Types

| Type | What it measures | Score range |
|---|---|---|
| `file-diff` | File content matches expected state | 0.0-1.0 |
| `contains` / `not_contains` | Output text contains/excludes strings | 0.0-1.0 |
| `test-pass` | Run a test command, check exit code | 0.0-1.0 |
| `turn-count` | Number of turns relative to ideal | 0.0-1.0 |
| `tool-allowlist` | Only expected tools were used | 0.0-1.0 |
| `tool-count` | Total tool invocations vs expected | 0.0-1.0 |
| `token-budget` | Total tokens consumed vs budget | 0.0-1.0 |
| `no-destructive` | No dangerous patterns in tool calls | 0.0-1.0 |
| `llm-judge` | LLM evaluates output quality with rubric | 0.0-1.0 |
| `exit-code` | Agent process exit code | 0.0 or 1.0 |

### Score Card

Each eval run produces a score card:

```json
{
  "agent_id": "styrene.coding-agent",
  "suite": "coding",
  "timestamp": "2026-04-15T14:00:00Z",
  "scenarios": [
    {
      "name": "fix-typo",
      "difficulty": 1,
      "scores": {
        "correctness": 1.0,
        "efficiency": 0.9,
        "tool-discipline": 1.0,
        "safety": 1.0
      },
      "weighted_score": 0.97,
      "turns": 3,
      "tokens": 1200,
      "duration_secs": 15,
      "passed": true
    }
  ],
  "aggregate": {
    "total_score": 0.85,
    "pass_rate": 0.90,
    "avg_turns": 4.2,
    "avg_tokens": 2100,
    "by_difficulty": {
      "1": 0.95,
      "2": 0.82,
      "3": 0.71
    },
    "by_dimension": {
      "correctness": 0.88,
      "efficiency": 0.79,
      "tool-discipline": 0.92,
      "safety": 1.0
    }
  }
}
```

## Harness Architecture

```
omegon eval --agent styrene.coding-agent --suite evals/coding/suite.toml
    │
    ├─ Parse suite.toml → list of scenarios
    ├─ For each scenario:
    │   ├─ Set up fixture (clone repo, create files)
    │   ├─ Spawn daemon: omegon serve --agent <id>
    │   ├─ Wait for /api/readyz
    │   ├─ POST /api/events with scenario prompt
    │   ├─ Poll /api/state until turn completes
    │   ├─ Collect: conversation, tool calls, tokens, turns
    │   ├─ Run scorers against collected data
    │   ├─ Kill daemon
    │   └─ Record scenario result
    ├─ Aggregate scores
    └─ Output score card (JSON + human-readable summary)
```

The harness reuses the daemon blackbox test pattern (`tests/daemon_serve_blackbox.rs`): spawn a real omegon process, communicate via HTTP, parse startup JSON for auth token.

## Implementation Plan

### New files

1. **`core/crates/omegon/src/eval/mod.rs`** — Eval harness module
2. **`core/crates/omegon/src/eval/scenario.rs`** — Scenario parser (TOML)
3. **`core/crates/omegon/src/eval/scorer.rs`** — Scorer implementations
4. **`core/crates/omegon/src/eval/harness.rs`** — Daemon lifecycle + event injection
5. **`core/crates/omegon/src/eval/report.rs`** — Score card generation

### CLI integration

```
omegon eval --agent <id> --suite <path>     # Run eval suite
omegon eval --agent <id> --scenario <path>  # Run single scenario
omegon eval --list-suites                   # List available suites
omegon eval --compare <card1> <card2>       # Compare two score cards
```

### Example eval suites to ship

1. **`evals/coding/`** — 10 scenarios across difficulty 1-3
   - Fix typo, add docstring, write test, refactor function, fix bug from issue description
2. **`evals/infra/`** — 8 scenarios
   - List pods, describe failing deployment, check cert expiry, write helm values
3. **`evals/chat/`** — 5 scenarios
   - Summarize text, answer factual question, triage bug report, extract action items

## Ranking System

Score cards are stored in `$OMEGON_HOME/eval-results/` and indexed by agent_id + suite + timestamp. The catalog displays:

```
CATALOG                             CODING    INFRA    OVERALL
styrene.coding-agent v1.0.0         0.85      —        0.85
styrene.infra-engineer v1.0.0       —         0.79     0.79
community.python-specialist v0.3.0  0.91      —        0.91
```

Auspex uses scores to:
- Auto-select the best agent for a task type
- Flag regression when a bundle update drops its score
- Gate catalog submissions: must pass baseline threshold (e.g., >0.6)

## Continuous Improvement Loop

```
Author publishes bundle → CI runs eval suite → score card generated
    │
    ├─ Score drops below threshold → PR blocked
    ├─ Score improves → PR approved with score diff
    └─ New scenario added → all bundles re-evaluated
```

The verify-bundle.yml workflow already screens for safety. The eval harness adds effectiveness scoring on top.
