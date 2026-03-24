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

# Pull latest and build release (handles Cargo.lock conflicts from version bumps)
update:
    git checkout -- core/Cargo.lock 2>/dev/null || true
    git checkout -- .omegon/history 2>/dev/null || true
    git pull --rebase
    cd core && cargo build --release -p omegon
    @echo "Updated to $(cd core && ./target/release/omegon --version 2>/dev/null || echo 'build failed')"

# Full release build (fat LTO, single codegen unit — slow link, smallest binary)
build-release:
    cd core && cargo build --release -p omegon

# Symlink build binary so rebuilds are instantly live.
# Targets /opt/homebrew/bin if the existing symlink lives there, else ~/.cargo/bin.
link:
    #!/usr/bin/env bash
    set -euo pipefail
    SRC="{{justfile_directory()}}/core/target/release/omegon"
    if [ -L /opt/homebrew/bin/omegon ] || [ -f /opt/homebrew/bin/omegon ]; then
        ln -sf "$SRC" /opt/homebrew/bin/omegon
        echo "Linked /opt/homebrew/bin/omegon → $SRC"
    else
        ln -sf "$SRC" ~/.cargo/bin/omegon
        echo "Linked ~/.cargo/bin/omegon → $SRC"
    fi
    "$SRC" --version 2>/dev/null || echo "(build first with: just build)"

# Run the built binary directly (no recompile)
run *args:
    core/target/release/omegon {{args}}

# ─── Release ─────────────────────────────────────────────────

# Cut a release candidate: bump rc.N, build, test, commit, tag.
# Push the tag manually to publish: git push origin <tag>
rc:
    #!/usr/bin/env bash
    set -euo pipefail

    # Refuse to run with uncommitted changes
    if [ -n "$(git status --porcelain -- core/)" ]; then
        echo "✗ Uncommitted changes in core/. Commit or stash first."
        exit 1
    fi

    # Read current version
    CURRENT=$(grep '^version = ' core/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    echo "Current version: $CURRENT"

    # Bump RC number
    if echo "$CURRENT" | grep -q '\-rc\.'; then
        # Already an RC — increment the number
        BASE=$(echo "$CURRENT" | sed 's/-rc\.[0-9]*//')
        RC_NUM=$(echo "$CURRENT" | sed 's/.*-rc\.//')
        NEW_RC=$((RC_NUM + 1))
        NEW_VERSION="${BASE}-rc.${NEW_RC}"
    else
        # Stable version — start rc.1 on next patch
        IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"
        NEW_VERSION="${MAJOR}.${MINOR}.$((PATCH + 1))-rc.1"
    fi

    echo "New version: $NEW_VERSION"

    # Update Cargo.toml
    sed -i '' "s/^version = \"${CURRENT}\"/version = \"${NEW_VERSION}\"/" core/Cargo.toml

    # Build
    echo "Building..."
    cd core && cargo build --release -p omegon 2>&1 | tail -3
    cd ..

    # Test
    echo "Testing..."
    cd core && cargo test -p omegon 2>&1 | tail -3
    cd ..

    # Commit and tag
    git add core/Cargo.toml core/Cargo.lock
    git commit -m "chore(release): ${NEW_VERSION}"
    git tag "v${NEW_VERSION}"

    echo ""
    echo "✓ ${NEW_VERSION} — built, tested, committed, tagged."
    echo "  To publish: git push origin v${NEW_VERSION}"

# Cut a stable release: strip -rc.N, build, test, commit, tag.
release:
    #!/usr/bin/env bash
    set -euo pipefail

    if [ -n "$(git status --porcelain -- core/)" ]; then
        echo "✗ Uncommitted changes in core/. Commit or stash first."
        exit 1
    fi

    CURRENT=$(grep '^version = ' core/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

    if ! echo "$CURRENT" | grep -q '\-rc\.'; then
        echo "✗ Current version ($CURRENT) is not an RC. Bump to an RC first with: just rc"
        exit 1
    fi

    NEW_VERSION=$(echo "$CURRENT" | sed 's/-rc\.[0-9]*//')
    echo "Releasing: $CURRENT → $NEW_VERSION"

    sed -i '' "s/^version = \"${CURRENT}\"/version = \"${NEW_VERSION}\"/" core/Cargo.toml

    echo "Building..."
    cd core && cargo build --release -p omegon 2>&1 | tail -3
    cd ..

    echo "Testing..."
    cd core && cargo test -p omegon 2>&1 | tail -3
    cd ..

    git add core/Cargo.toml core/Cargo.lock
    git commit -m "chore(release): ${NEW_VERSION}"
    git tag "v${NEW_VERSION}"

    echo ""
    echo "✓ ${NEW_VERSION} — built, tested, committed, tagged."
    echo "  To publish: git push origin v${NEW_VERSION}"

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
