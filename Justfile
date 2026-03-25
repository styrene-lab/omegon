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

# Link the newest built binary onto $PATH so it can be run as `omegon` system-wide.
# Prefers release over dev-release (just rc builds release; just update builds dev-release).
# Resolution order: /opt/homebrew/bin (macOS+Homebrew) → /usr/local/bin → ~/.local/bin
link:
    #!/usr/bin/env bash
    set -e
    REL="$(pwd)/core/target/release/omegon"
    DEV="$(pwd)/core/target/dev-release/omegon"
    # Pick whichever exists and is newer
    if [ -f "$REL" ] && [ -f "$DEV" ]; then
        if [ "$REL" -nt "$DEV" ]; then BINARY="$REL"; else BINARY="$DEV"; fi
    elif [ -f "$REL" ]; then
        BINARY="$REL"
    elif [ -f "$DEV" ]; then
        BINARY="$DEV"
    else
        echo "No binary found — run 'just rc' or 'just update' first"
        exit 1
    fi
    # Pick first writable candidate in PATH-order
    if [ -d "/opt/homebrew/bin" ] && [ -w "/opt/homebrew/bin" ]; then
        DEST="/opt/homebrew/bin/omegon"
    elif [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
        DEST="/usr/local/bin/omegon"
    else
        mkdir -p "$HOME/.local/bin"
        DEST="$HOME/.local/bin/omegon"
        if ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
            echo "⚠  ~/.local/bin is not in \$PATH — add it to your shell profile:"
            echo "   export PATH=\"\$HOME/.local/bin:\$PATH\""
        fi
    fi
    ln -sf "$BINARY" "$DEST"
    echo "✓ omegon → $DEST"
    "$DEST" --version

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

# Run the built binary directly (no recompile)
run *args:
    core/target/dev-release/omegon {{args}}

# ─── Release ─────────────────────────────────────────────────

# Cut a release candidate: bump rc.N, test, commit, tag, build, sign.
# Push the tag to trigger CI: git push origin main --tags
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

    # Test first (faster than build, catches errors early)
    echo "Testing..."
    cd core && cargo test -p omegon 2>&1 | tail -3
    cd ..

    # Commit and tag BEFORE final build so the binary has the right sha
    git add core/Cargo.toml core/Cargo.lock
    git commit -m "chore(release): ${NEW_VERSION}"
    git tag "v${NEW_VERSION}"

    # Build release (now the tag and commit are baked into the binary)
    echo "Building..."
    cd core && cargo build --release -p omegon 2>&1 | tail -3
    cd ..

    # Code sign — run `just sign` separately if YubiKey isn't in env
    BINARY="core/target/release/omegon"
    if [ -n "${SMARTCARD_PIN:-}" ]; then
        SCAN=$("$HOME/.cargo/bin/rcodesign" smartcard-scan 2>/dev/null || true)
        if echo "$SCAN" | grep -q "Developer ID Application"; then
            echo "Signing with Apple Developer ID (YubiKey)..."
            echo "⚡ Touch YubiKey when it blinks"
            "$HOME/.cargo/bin/rcodesign" sign \
                --smartcard-slot 9c \
                --smartcard-pin-env SMARTCARD_PIN \
                --code-signature-flags runtime "$BINARY"
            echo "Signed with Developer ID Application (YubiKey)"
        fi
    elif security find-identity -v -p codesigning 2>/dev/null | grep -q "Omegon Local Dev"; then
        codesign -f -s "Omegon Local Dev" --identifier "dev.styrene.omegon" "$BINARY"
        echo "Signed with Omegon Local Dev certificate"
    else
        codesign -f -s - --identifier "dev.styrene.omegon" "$BINARY" 2>/dev/null || true
        echo "Ad-hoc signed (run 'just sign' to sign with Developer ID)"
    fi

    echo ""
    echo "✓ ${NEW_VERSION} — tested, committed, tagged, built."
    echo "  To publish: git push origin main --tags"

# Cut a stable release: strip -rc.N, test, commit, tag, build.
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

    echo "Testing..."
    cd core && cargo test -p omegon 2>&1 | tail -3
    cd ..

    git add core/Cargo.toml core/Cargo.lock
    git commit -m "chore(release): ${NEW_VERSION}"
    git tag "v${NEW_VERSION}"

    echo "Building..."
    cd core && cargo build --release -p omegon 2>&1 | tail -3
    cd ..

    echo ""
    echo "✓ ${NEW_VERSION} — tested, committed, tagged, built."
    echo "  To publish: git push origin main --tags"

# Sign the release binary with Apple Developer ID (YubiKey).
# Interactive — prompts for PIN and touch.
# Run after `just rc` if SMARTCARD_PIN wasn't set.
sign:
    #!/usr/bin/env bash
    set -euo pipefail
    BINARY="core/target/release/omegon"
    if [ ! -f "$BINARY" ]; then
        echo "✗ No binary at $BINARY — run 'just rc' first."
        exit 1
    fi

    SCAN=$("$HOME/.cargo/bin/rcodesign" smartcard-scan 2>/dev/null || true)
    if ! echo "$SCAN" | grep -q "Developer ID Application"; then
        echo "✗ No Developer ID Application certificate found on YubiKey."
        echo "  Insert YubiKey and check: rcodesign smartcard-scan"
        exit 1
    fi

    echo "Signing with Apple Developer ID (YubiKey)..."
    echo "⚡ Enter PIN when prompted, then touch YubiKey when it blinks"
    echo ""
    "$HOME/.cargo/bin/rcodesign" sign \
        --smartcard-slot 9c \
        --code-signature-flags runtime \
        "$BINARY"

    echo ""
    echo "Verifying..."
    codesign -dvvv "$BINARY" 2>&1 | grep -E "Authority|Team|Signature|Identifier"
    echo ""
    echo "✓ Signed."

# One-time setup: create a self-signed code signing certificate.
# Prevents macOS keychain permission prompts on every RC build.
# Requires sudo (to add trusted cert to System keychain).
setup-signing:
    #!/usr/bin/env bash
    set -euo pipefail

    if security find-identity -v -p codesigning 2>/dev/null | grep -q "Omegon Local Dev"; then
        echo "✓ Omegon Local Dev signing identity already exists"
        security find-identity -v -p codesigning | grep "Omegon"
        exit 0
    fi

    echo "Creating self-signed code signing certificate: Omegon Local Dev"
    echo "This is a one-time setup. You'll be asked for your password (sudo)."
    echo ""

    TMPDIR=$(mktemp -d)
    cat > "$TMPDIR/cert.cfg" <<'CERT'
    [ req ]
    default_bits = 2048
    prompt = no
    default_md = sha256
    distinguished_name = dn
    x509_extensions = v3_code_sign

    [ dn ]
    CN = Omegon Local Dev
    O = Styrene Lab

    [ v3_code_sign ]
    keyUsage = digitalSignature
    extendedKeyUsage = codeSigning
    basicConstraints = CA:false
    CERT

    openssl req -x509 -newkey rsa:2048 \
        -keyout "$TMPDIR/key.pem" -out "$TMPDIR/cert.pem" \
        -days 3650 -nodes -config "$TMPDIR/cert.cfg" 2>/dev/null

    openssl pkcs12 -export -out "$TMPDIR/omegon.p12" \
        -inkey "$TMPDIR/key.pem" -in "$TMPDIR/cert.pem" \
        -passout pass: 2>/dev/null

    security import "$TMPDIR/omegon.p12" -k ~/Library/Keychains/login.keychain-db \
        -T /usr/bin/codesign -P "" 2>/dev/null

    echo "Adding certificate to System keychain as trusted (requires sudo)..."
    sudo security add-trusted-cert -d -r trustRoot \
        -k /Library/Keychains/System.keychain "$TMPDIR/cert.pem"

    rm -rf "$TMPDIR"

    echo ""
    if security find-identity -v -p codesigning 2>/dev/null | grep -q "Omegon Local Dev"; then
        echo "✓ Signing identity created. All future RC builds will be signed."
        security find-identity -v -p codesigning | grep "Omegon"
    else
        echo "⚠ Certificate imported but not showing as valid signing identity."
        echo "  Open Keychain Access → Certificates → Omegon Local Dev"
        echo "  → Get Info → Trust → Code Signing → Always Trust"
    fi

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

# ─── Merge safety ────────────────────────────────────────────

# Smoke test: verify critical subsystem invariants after a merge or release.
# Catches silent regressions (dropped files, missing providers, broken dashboard).
smoke:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Running post-merge smoke test..."
    FAIL=0

    # 1. Binary works
    VERSION=$(cd core && cargo run -q -- --version 2>/dev/null || echo "FAILED")
    echo "  Binary: $VERSION"
    [[ "$VERSION" == *"omegon"* ]] || { echo "  ✗ Binary doesn't produce version"; FAIL=1; }

    # 2. Test count doesn't drop below known floor
    TEST_COUNT=$(cd core && cargo test -p omegon 2>&1 | grep 'test result:' | awk '{print $4}')
    echo "  Tests: $TEST_COUNT"
    if [ "$TEST_COUNT" -lt 850 ]; then
        echo "  ✗ Test count ($TEST_COUNT) below safety floor (850)"
        FAIL=1
    fi

    # 3. Provider count (auth.rs PROVIDERS should have all inference providers)
    PROVIDER_COUNT=$(grep -c 'id: "' core/crates/omegon/src/auth.rs)
    echo "  Providers in auth.rs: $PROVIDER_COUNT"
    if [ "$PROVIDER_COUNT" -lt 15 ]; then
        echo "  ✗ Provider count ($PROVIDER_COUNT) below expected (15)"
        FAIL=1
    fi

    # 4. Tool count matches registry
    TOOL_COUNT=$(grep 'TOOL_COUNT: usize' core/crates/omegon/src/tool_registry.rs | grep -o '[0-9]*')
    echo "  Tools registered: $TOOL_COUNT"
    if [ "$TOOL_COUNT" -lt 45 ]; then
        echo "  ✗ Tool count ($TOOL_COUNT) below expected (45)"
        FAIL=1
    fi

    # 5. Key files haven't been gutted
    for file in core/crates/omegon/src/tui/dashboard.rs core/crates/omegon/src/providers.rs core/crates/omegon/src/tui/mod.rs; do
        LINES=$(wc -l < "$file" | tr -d ' ')
        MIN=900
        if [ "$LINES" -lt "$MIN" ]; then
            echo "  ✗ $file has only $LINES lines (expected >$MIN)"
            FAIL=1
        else
            echo "  $file: $LINES lines ✓"
        fi
    done

    # 6. No Node.js subprocess bridge
    if grep -q 'SubprocessBridge' core/crates/omegon/src/main.rs; then
        echo "  ✗ SubprocessBridge still referenced in main.rs"
        FAIL=1
    else
        echo "  No SubprocessBridge ✓"
    fi

    if [ "$FAIL" -eq 0 ]; then
        echo "✓ All smoke checks passed"
    else
        echo "✗ SMOKE TEST FAILED — review above"
        exit 1
    fi

# Check for line-count regressions in key files vs the previous tag.
# Warns on >20% drops — doesn't block, just signals.
line-check:
    #!/usr/bin/env bash
    set -euo pipefail
    PREV_TAG=$(git describe --tags --abbrev=0 HEAD~1 2>/dev/null || echo "")
    if [ -z "$PREV_TAG" ]; then
        echo "No previous tag to compare against — skipping"
        exit 0
    fi
    echo "Comparing line counts against $PREV_TAG..."
    WARN=0
    for file in core/crates/omegon/src/tui/dashboard.rs core/crates/omegon/src/providers.rs core/crates/omegon/src/tui/mod.rs core/crates/omegon/src/auth.rs core/crates/omegon/src/tui/instruments.rs; do
        NOW=$(wc -l < "$file" 2>/dev/null | tr -d ' ')
        PREV=$(git show "$PREV_TAG:$file" 2>/dev/null | wc -l | tr -d ' ')
        if [ "$PREV" -gt 0 ] && [ "$NOW" -gt 0 ]; then
            DROP=$(( (PREV - NOW) * 100 / PREV ))
            if [ "$DROP" -gt 20 ]; then
                echo "  ⚠ $file: $PREV → $NOW ($DROP% drop)"
                WARN=1
            fi
        fi
    done
    if [ "$WARN" -eq 0 ]; then
        echo "✓ No significant line-count drops"
    fi

# ─── Git / jj ───────────────────────────────────────────────

# Show recent jj log
log:
    jj log --no-graph -r 'ancestors(@, 15)' -T 'description.first_line() ++ "\n"'

# Push current work
push:
    jj bookmark set main -r @ && jj git push --bookmark main
