# Omegon — systems engineering harness
# Run `just` with no args to see all available recipes.

# Default: show available recipes
default:
    @just --list --unsorted

# ─── Rust (core/) ────────────────────────────────────────────

# Run all Rust tests
test-rust:
    cd core && timeout 300 cargo test || (echo "⚠ Tests exceeded 5 minute timeout" && exit 1)

# Run tests for a specific crate
test-crate crate:
    cd core && cargo test -p {{crate}}

# Run tests matching a pattern
test-filter pattern:
    cd core && cargo test -p omegon '{{pattern}}'

# Type check without building (fast feedback)
check:
    cd core && cargo check

# Full check: type check + clippy
lint:
    cd core && cargo check && cargo clippy -- -D warnings

# Build release binary
build:
    cd core && cargo build --release

# Pull latest and build (handles Cargo.lock conflicts from version bumps)
# Uses dev-release profile: optimized but fast link (~90% perf, ~10% link time)
update:
    git checkout -- core/Cargo.lock 2>/dev/null || true
    git checkout -- .omegon/history 2>/dev/null || true
    git pull --rebase
    cd core && cargo build --profile dev-release -p omegon
    @echo "Updated to $(cd core && ./target/dev-release/omegon --version 2>/dev/null || echo 'build failed')"

# Full release build (fat LTO, single codegen unit — slow link, smallest binary)
build-release:
    cd core && cargo build --release -p omegon

# Symlink release binary into ~/.cargo/bin so rebuilds are instantly live
link:
    ln -sf "{{justfile_directory()}}/core/target/release/omegon" ~/.cargo/bin/omegon
    @echo "Linked ~/.cargo/bin/omegon → core/target/release/omegon"
    @core/target/release/omegon --version 2>/dev/null || echo "(build first with: just build)"

# Run the built binary directly (no recompile)
run *args:
    core/target/dev-release/omegon {{args}}

# ─── TypeScript (omegon-pi) ─────────────────────────────────

# Run all TS tests
test-ts:
    cd ../omegon-pi && npx tsx --test tests/*.test.ts extensions/**/*.test.ts

# TS type check
typecheck:
    cd ../omegon-pi && npx tsc --noEmit

# Full TS check: typecheck + lifecycle + tests
check-ts:
    cd ../omegon-pi && npm run check

# ─── Armory (omegon-armory) ─────────────────────────────────

# Run armory plugin tests
test-armory:
    cd /tmp/omegon-armory && npx tsx --test tests/*.test.ts

# ─── Site ────────────────────────────────────────────────────

# Build the docs site
site-build:
    cd site && node scripts/build-design-tree.mjs && npx astro build

# Dev server for docs site
site-dev:
    cd site && npx astro dev

# ─── Combined ───────────────────────────────────────────────

# Run ALL tests (Rust + TS + armory)
test-all: test-rust test-ts

# Quick pre-commit check: Rust check + TS typecheck
pre-commit: check typecheck

# Full CI-equivalent: lint + all tests
ci: lint test-rust check-ts

# ─── Counts ─────────────────────────────────────────────────

# Show test counts across all suites
test-count:
    #!/usr/bin/env bash
    set -e
    echo "=== Rust ==="
    cd core && cargo test 2>&1 | grep "test result" | awk '{s+=$4; f+=$6} END {printf "  %d passed, %d failed\n", s, f}'
    echo "=== TypeScript ==="
    cd ../omegon-pi && npx tsx --test tests/*.test.ts extensions/**/*.test.ts 2>&1 | grep "^ℹ tests" | awk '{printf "  %s tests\n", $3}'
    echo "=== Armory ==="
    cd /tmp/omegon-armory && npx tsx --test tests/*.test.ts 2>&1 | grep "^ℹ tests" | awk '{printf "  %s tests\n", $3}' 2>/dev/null || echo "  (not available)"

# ─── Memory schema ──────────────────────────────────────────

# Regenerate the schema contract file after schema changes
schema-regen:
    cd core && cargo test -p omegon-memory schema_contract_generate -- --ignored

# Verify schema contract is current
schema-check:
    cd core && cargo test -p omegon-memory schema_contract_is_current

# ─── Secrets ────────────────────────────────────────────────

# Run secrets crate tests
test-secrets:
    cd core && cargo test -p omegon-secrets

# ─── MCP ────────────────────────────────────────────────────

# Run MCP transport tests
test-mcp:
    cd core && cargo test -p omegon plugins::mcp

# ─── Plugins ────────────────────────────────────────────────

# Run all plugin tests (armory + mcp + registry)
test-plugins:
    cd core && cargo test -p omegon plugins

# ─── Design tree ────────────────────────────────────────────

# Show design tree status distribution
tree-status:
    @grep -h "^status:" docs/*.md 2>/dev/null | sort | uniq -c | sort -rn

# Show active work (exploring + implementing + decided)
tree-active:
    #!/usr/bin/env bash
    echo "=== Exploring ===" && grep -l "^status: exploring" docs/*.md | xargs -I{} basename {} .md | sort
    echo "" && echo "=== Implementing ===" && grep -l "^status: implementing" docs/*.md | xargs -I{} basename {} .md | sort
    echo "" && echo "=== Decided (ready) ===" && grep -l "^status: decided" docs/*.md | xargs -I{} basename {} .md | sort

# Count design tree nodes
tree-count:
    @echo "$(ls docs/*.md 2>/dev/null | wc -l | tr -d ' ') nodes"

# ─── Supply chain ────────────────────────────────────────────

# Generate CycloneDX SBOM from Cargo.lock
sbom:
    cd core && cargo cyclonedx --format json --output-cdx
    @echo "SBOM written to core/bom.json"

# Verify a downloaded binary's cosign signature
verify-sig binary:
    cosign verify-blob "{{binary}}" \
      --signature "{{binary}}.sig" \
      --certificate "{{binary}}.pem" \
      --certificate-identity-regexp "github.com/styrene-lab/omegon" \
      --certificate-oidc-issuer "https://token.actions.githubusercontent.com"

# ─── Git / jj ───────────────────────────────────────────────

# Show recent jj log
log:
    jj log --no-graph -r 'ancestors(@, 15)' -T 'description.first_line() ++ "\n"'

# Push current work
push:
    jj bookmark set main -r @ && jj git push --bookmark main
