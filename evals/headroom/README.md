# Headroom eval packs

Headroom eval packs let contributors add coverage for languages and document formats outside the core maintainer workflow without expanding the core release contract.

Core owns the harness, schema, and owned packs. Community packs are welcome, but they are optional unless a core maintainer explicitly promotes them.

## Pack tiers

- `core`: maintained by the Omegon project; may participate in release/default-on decisions.
- `community`: maintained by contributors; validated when touched and useful for visibility, but not a core release gate.
- `experimental`: useful but allowed to drift; failures should not block unrelated work.
- `deprecated`: retained for reproducibility only.

## Contribution contract

A pack contribution should include:

1. `pack.toml` with owner/tier/policy metadata.
2. `corpus.json` with reproducible bounded sources.
3. Source license/provenance notes.
4. Realistic `min_savings_percent` and `max_restored_facts` policy.
5. Required facts that are meaningful for the domain, not arbitrary noise.

Community packs do not imply core maintainer ownership. If a pack is outside core project use, contributors own the domain tuning unless core explicitly adopts it.

## Local validation

```bash
just headroom-pack-eval evals/headroom/packs/core/omegon
```

Strict validation uses the pack policy:

```bash
just headroom-pack-eval-strict evals/headroom/packs/core/omegon
```

Generated samples and fixtures live under `.tmp/headroom/` and must not be committed.
