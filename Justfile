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
# Resolution order: /usr/local/bin → ~/.local/bin
link:
    #!/usr/bin/env bash
    set -euo pipefail
    # Reconcile jj+git colocated state before HEAD checks.
    ./scripts/sync-jj-to-git.sh
    if ! git symbolic-ref -q HEAD >/dev/null 2>&1 && [ "${OMEGON_ALLOW_DETACHED_LINK:-0}" != "1" ]; then
        echo "✗ Detached HEAD. Refusing to link from an unattached commit."
        echo "  Check out main (or set OMEGON_ALLOW_DETACHED_LINK=1 for an intentional tagged/worktree build)."
        exit 1
    fi
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
    if [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
        DEST="/usr/local/bin/omegon"
        ALT="$HOME/.local/bin/omegon"
    else
        mkdir -p "$HOME/.local/bin"
        DEST="$HOME/.local/bin/omegon"
        ALT="/usr/local/bin/omegon"
        if ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
            echo "⚠  ~/.local/bin is not in \$PATH — add it to your shell profile:"
            echo "   export PATH=\"\$HOME/.local/bin:\$PATH\""
        fi
    fi
    # Remove stale install at the other location to prevent bash hash table confusion
    if [ -e "$ALT" ] || [ -L "$ALT" ]; then
        rm -f "$ALT"
        echo "  removed stale install at $ALT"
    fi
    ln -sf "$BINARY" "$DEST"
    echo "✓ omegon → $DEST"
    echo "  run 'hash -d omegon 2>/dev/null || true' if your shell cached the old path"
    "$DEST" --version

# Pull latest and build (handles Cargo.lock conflicts from version bumps)
# Uses dev-release profile: optimized but fast link (~90% perf, ~10% link time)
update:
    git checkout -- core/Cargo.lock 2>/dev/null || true
    git checkout -- .omegon/history 2>/dev/null || true
    git pull --rebase
    cd core && cargo build --profile dev-release -p omegon
    @echo "Updated to $(cd core && ./target/dev-release/omegon --version 2>/dev/null || echo 'build failed')"

# Build and link a specific tagged RC/stable release into a durable local worktree.
# This is the supported way to pin the installed CLI to a release tag.
link-tag tag:
    #!/usr/bin/env bash
    set -euo pipefail

    TAG="{{tag}}"
    if [ -z "$TAG" ]; then
        echo "✗ Usage: just link-tag vX.Y.Z[-rc.N]"
        exit 1
    fi
    case "$TAG" in
        v*) ;;
        *)
            echo "✗ Tag must start with 'v' (got: $TAG)"
            exit 1
            ;;
    esac

    if ! git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
        echo "✗ Tag $TAG does not exist locally. Fetch or create it first."
        exit 1
    fi

    VERSION="${TAG#v}"
    WT="$(pwd)/.omegon/release-worktrees/$TAG"
    BINARY="$WT/core/target/release/omegon"

    mkdir -p "$(pwd)/.omegon/release-worktrees"

    echo "Preparing durable worktree for $TAG..."
    if [ ! -d "$WT/.git" ] && [ ! -f "$WT/.git" ]; then
        echo "  creating worktree at $WT"
        git worktree add --detach "$WT" "$TAG"
    else
        echo "  reusing existing worktree at $WT"
    fi

    MANIFEST_VERSION=$(grep '^version = ' "$WT/core/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
    if [ "$MANIFEST_VERSION" != "$VERSION" ]; then
        echo "✗ Tag/version mismatch:"
        echo "  tag:      $TAG"
        echo "  manifest: $MANIFEST_VERSION"
        exit 1
    fi

    if [ -x "$BINARY" ]; then
        EXISTING_VERSION=$($BINARY --version | head -1 || true)
        echo "  existing binary: $EXISTING_VERSION"
        if echo "$EXISTING_VERSION" | grep -q "$VERSION"; then
            echo "✓ Existing tagged binary already matches $VERSION — skipping rebuild"
        else
            echo "Building $TAG in durable worktree..."
            echo "  binary version mismatch; rebuilding release binary"
            (cd "$WT/core" && cargo build --release -p omegon)
        fi
    else
        echo "Building $TAG in durable worktree..."
        echo "  no existing tagged binary found; building release binary"
        (cd "$WT/core" && cargo build --release -p omegon)
    fi

    echo "Linking installed omegon to $TAG..."
    (cd "$WT" && OMEGON_ALLOW_DETACHED_LINK=1 just link)

    BINARY_VERSION=$($BINARY --version | head -1)
    if ! echo "$BINARY_VERSION" | grep -q "$VERSION"; then
        echo "✗ Built binary version mismatch: $BINARY_VERSION"
        exit 1
    fi

    echo "✓ Linked $TAG from $WT"
    echo "  $BINARY_VERSION"

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

    # Reconcile jj+git colocated state: fast-forward refs/heads/main to include
    # any jj commits that the harness created, then re-attach HEAD.
    # Must run before any guard that reads git branch/HEAD state.
    ./scripts/sync-jj-to-git.sh

    # Refuse detached HEAD or non-main release cuts.
    BRANCH=$(git branch --show-current)
    if [ -z "$BRANCH" ]; then
        echo "✗ Detached HEAD. Check out main before cutting an RC."
        exit 1
    fi
    if [ "$BRANCH" != "main" ]; then
        echo "✗ RC cuts must run from main. Current branch: $BRANCH"
        exit 1
    fi
    HEAD_SHA=$(git rev-parse HEAD)
    MAIN_SHA=$(git rev-parse refs/heads/main)
    if [ "$HEAD_SHA" != "$MAIN_SHA" ]; then
        echo "✗ HEAD is not the tip of main. Check out main and retry."
        exit 1
    fi

    # Refuse to run with uncommitted changes (core/ and milestones)
    DIRTY=$(git status --porcelain -- core/ .omegon/milestones.json)
    if [ -n "$DIRTY" ]; then
        echo "✗ Uncommitted changes in core/ or milestones. Commit or stash first."
        echo "$DIRTY"
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

    # Update milestone tracking
    ./scripts/milestone-update.sh rc "$NEW_VERSION"

    # Audit lifecycle drift before cutting the RC
    echo "Lifecycle audit..."
    cd core && cargo run --quiet -p omegon -- doctor
    cd ..

    # Test first (faster than build, catches errors early)
    echo "Testing..."
    # Auto-accept snapshot updates — version bump invalidates version-string snapshots
    cd core && INSTA_UPDATE=always cargo test -p omegon 2>&1 | tail -3
    cd ..

    # Commit and tag BEFORE final build so the binary has the right sha
    git add core/Cargo.toml core/Cargo.lock .omegon/milestones.json
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

    # Reconcile jj+git colocated state before any guards read HEAD.
    ./scripts/sync-jj-to-git.sh

    BRANCH=$(git branch --show-current)
    if [ -z "$BRANCH" ]; then
        echo "✗ Detached HEAD. Check out main before cutting a stable release."
        exit 1
    fi
    if [ "$BRANCH" != "main" ]; then
        echo "✗ Stable releases must run from main. Current branch: $BRANCH"
        exit 1
    fi
    HEAD_SHA=$(git rev-parse HEAD)
    MAIN_SHA=$(git rev-parse refs/heads/main)
    if [ "$HEAD_SHA" != "$MAIN_SHA" ]; then
        echo "✗ HEAD is not the tip of main. Check out main and retry."
        exit 1
    fi

    DIRTY=$(git status --porcelain -- core/ .omegon/milestones.json)
    if [ -n "$DIRTY" ]; then
        echo "✗ Uncommitted changes in core/ or milestones. Commit or stash first."
        echo "$DIRTY"
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

    # Mark milestone as released
    ./scripts/milestone-update.sh release "$NEW_VERSION"

    echo "Testing..."
    cd core && cargo test -p omegon 2>&1 | tail -3
    cd ..

    git add core/Cargo.toml core/Cargo.lock .omegon/milestones.json
    git commit -m "chore(release): ${NEW_VERSION}"
    git tag "v${NEW_VERSION}"

    echo "Building..."
    cd core && cargo build --release -p omegon 2>&1 | tail -3
    cd ..

    echo ""
    echo "✓ ${NEW_VERSION} — tested, committed, tagged, built."
    echo "  To publish: git push origin main v${NEW_VERSION}"

    # Open the next RC cycle immediately so dev builds aren't mislabelled as the
    # just-shipped stable release.  No rebuild needed — this is just a version bump.
    IFS='.' read -r MAJOR MINOR PATCH <<< "$NEW_VERSION"
    NEXT_PATCH="${MAJOR}.${MINOR}.$((PATCH + 1))"
    NEXT_RC="${NEXT_PATCH}-rc.1"
    echo ""
    echo "Opening next cycle: $NEXT_RC"
    sed -i '' "s/^version = \"${NEW_VERSION}\"/version = \"${NEXT_RC}\"/" core/Cargo.toml

    # Open next milestone
    ./scripts/milestone-update.sh open "$NEXT_PATCH"

    cd core && cargo check -p omegon -q 2>&1 | tail -1; cd ..
    git add core/Cargo.toml core/Cargo.lock .omegon/milestones.json
    git commit -m "chore(release): ${NEXT_RC}"
    echo "✓ Bumped to ${NEXT_RC} — branch is now open for the next cycle."
    echo "  Publish stable: git push origin main v${NEW_VERSION}"
    echo "  (RC tag is created by 'just rc' when there's something to release)"
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
    if [ -n "${SMARTCARD_PIN:-}" ]; then
        echo "Using SMARTCARD_PIN from environment"
        echo "⚡ Touch YubiKey when it blinks"
        "$HOME/.cargo/bin/rcodesign" sign \
            --smartcard-slot 9c \
            --smartcard-pin-env SMARTCARD_PIN \
            --code-signature-flags runtime \
            "$BINARY"
    else
        echo "⚡ Enter PIN when prompted, then touch YubiKey when it blinks"
        echo ""
        "$HOME/.cargo/bin/rcodesign" sign \
            --smartcard-slot 9c \
            --code-signature-flags runtime \
            "$BINARY"
    fi

    echo ""
    echo "Verifying signature..."
    codesign -dvvv "$BINARY" 2>&1 | grep -E "Authority|Team|Signature|Identifier"

    # Submit for notarization (non-blocking)
    if xcrun notarytool history --keychain-profile "omegon" >/dev/null 2>&1; then
        echo ""
        echo "Submitting for Apple notarization..."
        NOTARY_ZIP="${BINARY}.zip"
        ditto -c -k --keepParent "$BINARY" "$NOTARY_ZIP"

        # Submit without --wait — returns immediately with a submission ID
        SUBMIT_OUT=$(xcrun notarytool submit "$NOTARY_ZIP" \
            --keychain-profile "omegon" 2>&1)
        echo "$SUBMIT_OUT"

        SUBMISSION_ID=$(echo "$SUBMIT_OUT" | grep -o '[0-9a-f-]\{36\}' | head -1)
        rm -f "$NOTARY_ZIP"

        if [ -n "$SUBMISSION_ID" ]; then
            echo ""
            echo "Notarization submitted: $SUBMISSION_ID"
            echo "Check status:  xcrun notarytool info $SUBMISSION_ID --keychain-profile omegon"
            echo "View log:      xcrun notarytool log $SUBMISSION_ID --keychain-profile omegon"
            echo "Full history:  xcrun notarytool history --keychain-profile omegon"
            echo ""
            echo "✓ Signed. Notarization in progress (Apple processes in 1-15 minutes)."
            echo "  Gatekeeper will pass the binary once Apple approves it."
        else
            echo ""
            echo "✓ Signed. Notarization submission may have failed — check output above."
        fi
    else
        echo ""
        echo "✓ Signed (notarization skipped — run 'just setup-notarize' to enable)."
    fi

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

# Publish: push to origin, trigger CI, build docs, link binary.
# Run after `just sign` (or `just rc` if ad-hoc signing is fine).
# Flow: just rc → just sign → just publish
publish:
    #!/usr/bin/env bash
    set -euo pipefail

    BINARY="core/target/release/omegon"
    VERSION=$(grep '^version = ' core/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    TAG="v${VERSION}"

    echo "╭──────────────────────────────────╮"
    echo "│  Publishing omegon ${VERSION}    │"
    echo "╰──────────────────────────────────╯"

    # ── 1. Pre-flight checks ──────────────────────────────────
    if [ -n "$(git status --porcelain)" ]; then
        echo "✗ Uncommitted changes. Commit or stash first."
        exit 1
    fi

    if [ ! -f "$BINARY" ]; then
        echo "✗ No binary at $BINARY — run 'just rc' first."
        exit 1
    fi

    # Verify tag exists
    if ! git tag --list "$TAG" | grep -q "$TAG"; then
        echo "✗ Tag $TAG not found. Run 'just rc' or 'just release' first."
        exit 1
    fi

    # Verify binary version matches
    BINARY_VERSION=$("$BINARY" --version 2>/dev/null | head -1 || echo "unknown")
    if ! echo "$BINARY_VERSION" | grep -q "$VERSION"; then
        echo "✗ Binary version mismatch:"
        echo "  Cargo.toml: $VERSION"
        echo "  Binary:     $BINARY_VERSION"
        echo "  Rebuild with: just rc"
        exit 1
    fi

    echo "  Binary:  $BINARY_VERSION"

    # Check signing status
    SIGN_STATUS="unsigned"
    if codesign -dvvv "$BINARY" 2>&1 | grep -q "Developer ID"; then
        SIGN_STATUS="Developer ID (YubiKey)"
    elif codesign -dvvv "$BINARY" 2>&1 | grep -q "Omegon Local Dev"; then
        SIGN_STATUS="Omegon Local Dev (self-signed)"
    elif codesign -dvvv "$BINARY" 2>&1 | grep -q "Signature=adhoc"; then
        SIGN_STATUS="ad-hoc"
    fi
    echo "  Signing: $SIGN_STATUS"

    # ── 2. Push to origin (triggers CI: release, npm, site) ──
    echo ""
    echo "Pushing to origin..."
    git push origin main --tags

    echo ""
    echo "CI workflows triggered:"
    echo "  • release.yml  → GitHub Release with cosign-signed binaries"
    echo "  • site.yml     → omegon.styrene.dev docs rebuild"

    # ── 3. Build docs site locally (verification) ─────────────
    echo ""
    echo "Building docs site locally..."
    cd site
    node scripts/build-design-tree.mjs 2>/dev/null
    npx astro build 2>&1 | tail -5
    PAGES=$(find dist -name '*.html' | wc -l | tr -d ' ')
    echo "  Pages: $PAGES"
    cd ..

    # ── 4. Link the binary ────────────────────────────────────
    echo ""
    just link

    # ── 5. Run post-publish smoke test ────────────────────────
    echo ""
    just smoke

    # ── 6. Update Homebrew tap ───────────────────────────────
    echo ""
    just brew-tap

    # ── 7. Summary ────────────────────────────────────────────
    echo ""
    echo "╭──────────────────────────────────╮"
    echo "│  ✓ Published ${VERSION}          │"
    echo "╰──────────────────────────────────╯"
    echo ""
    echo "  Binary:   $(which omegon) → $BINARY"
    echo "  Signing:  $SIGN_STATUS"
    echo "  Docs:     $PAGES pages built → CI deploying to omegon.styrene.dev"
    echo "  Brew:     styrene-lab/homebrew-tap updated → brew upgrade omegon"
    echo "  Release:  github.com/styrene-lab/omegon/releases/tag/$TAG"
    echo ""
    echo "  Monitor CI: gh run list --limit 3"

# One-time setup: store App Store Connect API credentials for notarization.
setup-notarize:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Apple Notarization Setup"
    echo "========================"
    echo ""
    echo "1. Go to https://appstoreconnect.apple.com/access/integrations/api"
    echo "2. Generate a new API key (Developer role is sufficient)"
    echo "3. Download the .p8 file"
    echo ""
    read -p "Path to .p8 key file: " KEY_PATH
    read -p "Key ID (from App Store Connect): " KEY_ID
    read -p "Issuer ID (from App Store Connect): " ISSUER_ID
    echo ""
    echo "Add these to your shell profile:"
    echo "  export APPLE_API_KEY=\"$KEY_PATH\""
    echo "  export APPLE_API_KEY_ID=\"$KEY_ID\""
    echo "  export APPLE_API_ISSUER=\"$ISSUER_ID\""
    echo ""
    echo "Then run 'just sign' — notarization will be automatic."

# Create/update the Homebrew tap repo
brew-tap:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! gh repo view styrene-lab/homebrew-tap &>/dev/null 2>&1; then
        echo "Creating styrene-lab/homebrew-tap..."
        gh repo create styrene-lab/homebrew-tap --public \
            --description "Homebrew tap for Omegon — terminal-native AI agent harness"
    fi
    TMPDIR=$(mktemp -d)
    trap "rm -rf $TMPDIR" EXIT
    gh repo clone styrene-lab/homebrew-tap "$TMPDIR/tap" 2>/dev/null || {
        mkdir -p "$TMPDIR/tap"
        cd "$TMPDIR/tap"
        git init
        git remote add origin "https://github.com/styrene-lab/homebrew-tap.git"
    }
    mkdir -p "$TMPDIR/tap/Formula"
    cp homebrew/Formula/omegon.rb "$TMPDIR/tap/Formula/"
    cd "$TMPDIR/tap"
    git add -A
    git diff --cached --quiet || {
        git commit -m "formula: omegon $(grep 'version ' Formula/omegon.rb | head -1 | sed 's/.*\"\(.*\)\".*/\1/')"
        git push origin HEAD:main 2>/dev/null || git push -u origin main
    }
    echo ""
    echo "✓ Tap updated. Install with:"
    echo "  brew tap styrene-lab/tap"
    echo "  brew install omegon"

# ─── Cross-compile ───────────────────────────────────────────

# Build for Linux x86_64 (via zig cross-linker — no containers, no QEMU)
build-linux-amd64:
    cd core && cargo zigbuild --release --target x86_64-unknown-linux-gnu -p omegon
    @ls -lh core/target/x86_64-unknown-linux-gnu/release/omegon
    @file core/target/x86_64-unknown-linux-gnu/release/omegon

# Build for Linux aarch64 (via zig cross-linker)
build-linux-arm64:
    cd core && cargo zigbuild --release --target aarch64-unknown-linux-gnu -p omegon
    @ls -lh core/target/aarch64-unknown-linux-gnu/release/omegon
    @file core/target/aarch64-unknown-linux-gnu/release/omegon

# Build all release targets (macOS native + Linux via zig)
build-all: build build-linux-amd64 build-linux-arm64
    @echo ""
    @echo "Built:"
    @ls -lh core/target/release/omegon
    @ls -lh core/target/x86_64-unknown-linux-gnu/release/omegon
    @ls -lh core/target/aarch64-unknown-linux-gnu/release/omegon

# Package release archives for all targets
package:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(grep '^version = ' core/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    DIST="dist/v${VERSION}"
    mkdir -p "$DIST"

    package_target() {
        local TARGET=$1 BINARY=$2
        local ARCHIVE="omegon-${VERSION}-${TARGET}.tar.gz"
        strip "$BINARY" 2>/dev/null || llvm-strip "$BINARY" 2>/dev/null || true
        tar czf "${DIST}/${ARCHIVE}" -C "$(dirname "$BINARY")" omegon
        shasum -a 256 "${DIST}/${ARCHIVE}" >> "${DIST}/checksums.sha256"
        echo "  ${ARCHIVE} ($(du -h "${DIST}/${ARCHIVE}" | cut -f1))"
    }

    echo "Packaging v${VERSION}..."
    > "${DIST}/checksums.sha256"  # truncate

    # macOS arm64 (native build)
    if [ -f core/target/release/omegon ]; then
        package_target "aarch64-apple-darwin" "core/target/release/omegon"
    fi

    # Linux x86_64
    if [ -f core/target/x86_64-unknown-linux-gnu/release/omegon ]; then
        package_target "x86_64-unknown-linux-gnu" "core/target/x86_64-unknown-linux-gnu/release/omegon"
    fi

    # Linux aarch64
    if [ -f core/target/aarch64-unknown-linux-gnu/release/omegon ]; then
        package_target "aarch64-unknown-linux-gnu" "core/target/aarch64-unknown-linux-gnu/release/omegon"
    fi

    echo ""
    echo "Checksums:"
    cat "${DIST}/checksums.sha256"
    echo ""
    echo "Archives in ${DIST}/"

# Finalize a draft nightly release on this Mac using the YubiKey signing flow.
# Builds from the tagged source in a temporary worktree, signs/notarizes the
# macOS binary, uploads the signed archive + refreshed checksums, and publishes
# the draft GitHub release.
finalize-nightly tag='':
    #!/usr/bin/env bash
    set -euo pipefail

    if ! command -v gh >/dev/null 2>&1; then
        echo "✗ GitHub CLI (gh) is required. Install it and run 'gh auth login'."
        exit 1
    fi

    if ! gh auth status >/dev/null 2>&1; then
        echo "✗ gh is not authenticated. Run 'gh auth login'."
        exit 1
    fi

    if ! xcrun notarytool history --keychain-profile "omegon" >/dev/null 2>&1; then
        echo "✗ Apple notarization profile 'omegon' is not configured on this Mac."
        echo "  Run 'just setup-notarize' first."
        exit 1
    fi

    if [ -z "{{tag}}" ]; then
        TAG=$(gh release list --limit 50 --json tagName,isDraft,isPrerelease \
            --jq '.[] | select(.isDraft == true and .isPrerelease == true and (.tagName | contains("-nightly."))) | .tagName' | head -1)
        if [ -z "$TAG" ]; then
            echo "✗ No draft nightly release found. Pass an explicit tag: just finalize-nightly vX.Y.Z-nightly.YYYYMMDD"
            exit 1
        fi
    else
        TAG="{{tag}}"
    fi

    VERSION="${TAG#v}"
    TARGET="aarch64-apple-darwin"
    ARCHIVE="omegon-${VERSION}-${TARGET}.tar.gz"
    WORKTREE="$(mktemp -d /tmp/omegon-nightly-XXXXXX)"
    ARTIFACT_DIR="$(mktemp -d /tmp/omegon-nightly-artifacts-XXXXXX)"
    CHECKSUMS="$ARTIFACT_DIR/checksums.sha256"

    cleanup() {
        set +e
        if [ -d "$WORKTREE/.git" ] || [ -f "$WORKTREE/.git" ]; then
            git worktree remove --force "$WORKTREE" >/dev/null 2>&1 || true
        else
            rm -rf "$WORKTREE"
        fi
        rm -rf "$ARTIFACT_DIR"
    }
    trap cleanup EXIT

    echo "Preparing worktree for $TAG..."
    git fetch --tags origin "$TAG"
    git worktree add --detach "$WORKTREE" "$TAG"

    echo "Building macOS release binary..."
    (cd "$WORKTREE/core" && cargo build --release -p omegon)
    BINARY="$WORKTREE/core/target/release/omegon"

    echo "Signing with Apple Developer ID (YubiKey)..."
    if [ -n "${SMARTCARD_PIN:-}" ]; then
        echo "Using SMARTCARD_PIN from environment"
        echo "⚡ Touch YubiKey when it blinks"
        "$HOME/.cargo/bin/rcodesign" sign \
            --smartcard-slot 9c \
            --smartcard-pin-env SMARTCARD_PIN \
            --code-signature-flags runtime \
            "$BINARY"
    else
        echo "⚡ Enter PIN when prompted, then touch YubiKey when it blinks"
        "$HOME/.cargo/bin/rcodesign" sign \
            --smartcard-slot 9c \
            --code-signature-flags runtime \
            "$BINARY"
    fi

    echo "Verifying signature..."
    codesign -dvvv "$BINARY" 2>&1 | grep -E "Authority|Team|Signature|Identifier"

    echo "Submitting for Apple notarization (blocking)..."
    NOTARY_ZIP="$ARTIFACT_DIR/${ARCHIVE%.tar.gz}.zip"
    ditto -c -k --keepParent "$BINARY" "$NOTARY_ZIP"
    xcrun notarytool submit "$NOTARY_ZIP" --keychain-profile "omegon" --wait

    echo "Packaging signed macOS archive..."
    tar czf "$ARTIFACT_DIR/$ARCHIVE" -C "$(dirname "$BINARY")" omegon
    shasum -a 256 "$ARTIFACT_DIR/$ARCHIVE" > "$ARTIFACT_DIR/$ARCHIVE.sha256"

    echo "Refreshing release checksums..."
    gh release download "$TAG" -p 'checksums.sha256' -D "$ARTIFACT_DIR" >/dev/null 2>&1 || true
    if [ -f "$CHECKSUMS" ]; then
        grep -v "$TARGET" "$CHECKSUMS" > "$CHECKSUMS.tmp" || true
        mv "$CHECKSUMS.tmp" "$CHECKSUMS"
    else
        : > "$CHECKSUMS"
    fi
    cat "$ARTIFACT_DIR/$ARCHIVE.sha256" >> "$CHECKSUMS"

    echo "Uploading signed nightly assets to $TAG..."
    gh release upload "$TAG" \
        "$ARTIFACT_DIR/$ARCHIVE" \
        "$ARTIFACT_DIR/$ARCHIVE.sha256" \
        "$CHECKSUMS" \
        --clobber

    echo "Publishing draft nightly release..."
    gh release edit "$TAG" --draft=false

    echo ""
    echo "✓ Finalized nightly $TAG"
    echo "  Uploaded: $ARCHIVE"
    echo "  Release:  $(gh release view "$TAG" --json url --jq .url)"

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
