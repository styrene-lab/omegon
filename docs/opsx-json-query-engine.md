+++
id = "2dc9e6b3-bd27-4642-b480-48badd39cf7d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# JSON query engine for opsx-core — jsongrep integration for state.json querying

## Overview

Integrate jsongrep (DFA-based JSON query engine, Rust-native) as the query layer over .omegon/lifecycle/state.json. Replace ad-hoc Vec::iter().find() lookups with structured path queries. jsongrep is faster than jq/jmespath/jsonpath-rust/jql per benchmarks (https://micahkepe.com/blog/jsongrep/). Exposes both a CLI tool and a library crate — we'd use the library crate embedded in opsx-core for programmatic queries, and potentially expose a /query slash command for operator-facing ad-hoc queries against lifecycle state. This enables: milestone readiness queries, cross-node dependency analysis, filtered node lists by tag/status/priority, audit log searches — all without hand-writing Rust iterators for each query pattern.

## Open Questions

- Should jsongrep be a dependency of opsx-core (library crate) or a separate opsx-query crate that depends on opsx-core?
- What's the operator-facing syntax? jsongrep uses regular path expressions (e.g. $.nodes[*].state). Do we expose this directly or wrap it in domain-specific commands (/query nodes where status=exploring)?
- Should the query engine also index the audit log for time-range queries (e.g. 'all transitions in the last 24h')?
