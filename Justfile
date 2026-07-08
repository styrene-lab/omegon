# Omegon — systems engineering harness
# Run `just` with no args to see all available recipes.

# Platform-agnostic in-place sed. macOS sed requires a backup extension
# after -i; GNU sed does not. Using perl avoids the divergence entirely.
sedi := "perl -pi -e"
cargo := "cargo"

# Default: show available recipes
default:
    @just --list --unsorted

# ─── Bootstrap ───────────────────────────────────────────────

# Set up the development environment from scratch (Rust toolchain, build, link).
# Safe to re-run. Use `just bootstrap --check` to verify prerequisites only.
bootstrap *args:
    ./scripts/bootstrap.sh {{args}}

# ─── Hygiene ─────────────────────────────────────────

# Classify dirty tracked/runtime files before making scoped commits.
dirty-report:
    python3 scripts/dirty_report.py

# Gate on source-plane cleanliness while allowing live agent telemetry/state.
source-clean:
    python3 scripts/dirty_report.py --source-clean

# ─── Rust ────────────────────────────────────────────

# Run all Rust tests (CI/full-release gate; use test-changed/test-filter for focused local edits).
# Some tests intentionally exercise process-global state such as cwd/env/profile
# resolution; keep the full local gate serialized so those integration contracts
# do not race each other under libtest's default parallel scheduler.
test-rust:
    {{cargo}} test --workspace -- --test-threads=1

# Commit-time Rust validation for changed crates. This is the default local gate for
# focused commits; CI/release hardening still uses test-rust for the full workspace.
test-commit *args:
    just test-changed {{args}}

# Run tests for a specific crate
test-crate crate:
    {{cargo}} test -p {{crate}}

# Run tests matching a pattern
test-filter pattern:
    {{cargo}} test -p omegon '{{pattern}}'

# Show Rust workspace crates affected by changed paths.
affected *args:
    python3 scripts/affected_crates.py {{args}}

# Summarize Rust test/coupling hotspots without running tests.
test-profile *args:
    python3 scripts/test_profile.py {{args}}

# Run Python unit tests for developer tooling scripts.
test-dev-scripts:
    python3 -m unittest scripts/test_affected_crates.py scripts/test_test_profile.py scripts/test_dirty_report.py scripts/test_dirty_report_git.py
    python3 scripts/tests/test_omegon_launcher.py

# Check provider-published model context docs against the local registry.
provider-context-truth *args:
    python3 scripts/provider_context_truth.py {{args}}

# Check local provider drift and cheap upstream sources (no credentials required).
upstream-provider-check:
    python3 scripts/check_model_registry.py
    python3 scripts/check_upstream_versions.py
    python3 scripts/check_anthropic_models.py

# Run tests only for Rust crates affected by changed paths.
test-changed *args:
    #!/usr/bin/env bash
    set -euo pipefail
    crates="$(python3 scripts/affected_crates.py --format shell {{args}})"
    if [ -z "$crates" ]; then
        echo "No affected Rust crates; skipping cargo test."
        exit 0
    fi
    for crate in $crates; do
        echo "── cargo test -p $crate ──"
        {{cargo}} test -p "$crate"
    done

# Type check only Rust crates affected by changed paths.
check-changed *args:
    #!/usr/bin/env bash
    set -euo pipefail
    crates="$(python3 scripts/affected_crates.py --format shell {{args}})"
    if [ -z "$crates" ]; then
        echo "No affected Rust crates; skipping cargo check."
        exit 0
    fi
    for crate in $crates; do
        echo "── cargo check -p $crate ──"
        {{cargo}} check -p "$crate"
    done

# Run clippy only for Rust crates affected by changed paths.
clippy-changed *args:
    #!/usr/bin/env bash
    set -euo pipefail
    crates="$(python3 scripts/affected_crates.py --format shell {{args}})"
    if [ -z "$crates" ]; then
        echo "No affected Rust crates; skipping cargo clippy."
        exit 0
    fi
    {{cargo}} fmt --all --check
    for crate in $crates; do
        echo "── cargo clippy -p $crate --all-targets ──"
        {{cargo}} clippy -p "$crate" --all-targets -- -D warnings
    done

# Type check without building (fast feedback)
check:
    {{cargo}} check --workspace

# Full local lint gate for the entire workspace, including examples and tests.
lint:
    {{cargo}} fmt --all --check
    {{cargo}} check --workspace
    {{cargo}} clippy --workspace --all-targets -- -D warnings

# Structurally lint every docs/**/*.openapi.{yaml,yml} contract. Rust-native
# (no Node/Python toolchain); also runs in CI via the rust-integration job.
lint-openapi:
    {{cargo}} test -p omegon --test openapi_contract_lint -- --nocapture

# ─── Benchmarks ─────────────────────────────────────────────

# Run a quick token-efficiency benchmark. Writes per-turn snapshots to .tmp/bench/.
# Usage: just bench "read Cargo.toml and summarize the dependencies"
bench prompt:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p .tmp/bench
    ts=$(date +%Y%m%d-%H%M%S)
    out=".tmp/bench/run-${ts}.json"
    {{cargo}} run --release -p omegon -- bench run-task \
        --prompt "{{prompt}}" \
        --usage-json "../${out}" 2>&1 | tail -20
    echo ""
    echo "── Results: ${out} ──"
    python3 scripts/bench_summary.py "${out}" 2>/dev/null || echo "(install python3 for summary)"

# Compare two benchmark usage JSON files side by side.
# Usage: just bench-diff .tmp/bench/baseline.json .tmp/bench/current.json
bench-diff baseline current:
    python3 scripts/bench_diff.py {{baseline}} {{current}}

# ─── Benchmark Matrix ────────────────────────────────────────

# Capture a baseline: build release, run the canonical benchmark on all harnesses.
# Usage: just bench-baseline
bench-baseline:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "── Building release binary ──"
    {{cargo}} build --release -p omegon
    echo "── Capturing baseline ──"
    mkdir -p ai/benchmarks/runs/baseline
    python3 scripts/benchmark_matrix.py \
        ai/benchmarks/tasks/example-shadow-context.yaml \
        --root . \
        --out-dir ai/benchmarks/runs/baseline
    echo ""
    echo "Baseline saved to ai/benchmarks/runs/baseline/"
    ls -la ai/benchmarks/runs/baseline/*.json 2>/dev/null || echo "(no results — check harness output above)"

# Run the canonical benchmark and compare against baseline.
# Usage: just bench-compare
bench-compare:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ ! -d ai/benchmarks/runs/baseline ] || [ -z "$(ls ai/benchmarks/runs/baseline/*.json 2>/dev/null)" ]; then
        echo "No baseline found. Run 'just bench-baseline' first."
        exit 1
    fi
    echo "── Building release binary ──"
    {{cargo}} build --release -p omegon
    echo "── Running current ──"
    mkdir -p ai/benchmarks/runs/current
    python3 scripts/benchmark_matrix.py \
        ai/benchmarks/tasks/example-shadow-context.yaml \
        --root . \
        --out-dir ai/benchmarks/runs/current
    echo ""
    echo "── Comparison Report ──"
    python3 scripts/benchmark_harness.py \
        --report ai/benchmarks/runs/current/ \
        --baseline ai/benchmarks/runs/baseline/

# Run the full benchmark suite across all tasks and harnesses.
# Usage: just bench-full
bench-full:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "── Building release binary ──"
    {{cargo}} build --release -p omegon
    ts=$(date +%Y%m%d-%H%M%S)
    out="ai/benchmarks/runs/full-${ts}"
    mkdir -p "$out"
    echo "── Running full matrix ──"
    for task in ai/benchmarks/tasks/*.yaml; do
        name=$(basename "$task" .yaml)
        # Skip templates
        case "$name" in template-*) continue;; esac
        echo "  ▸ $name"
        python3 scripts/benchmark_matrix.py "$task" \
            --root . \
            --out-dir "$out" || echo "  ⚠ $name failed"
    done
    echo ""
    echo "── Report ──"
    python3 scripts/benchmark_harness.py --report "$out/"
    echo ""
    echo "Results: $out/"

# Run a single task on a specific harness.
# Usage: just bench-task example-shadow-context omegon
bench-task task harness:
    #!/usr/bin/env bash
    set -euo pipefail
    {{cargo}} build --release -p omegon
    ts=$(date +%Y%m%d-%H%M%S)
    out="ai/benchmarks/runs/single-${ts}"
    mkdir -p "$out"
    python3 scripts/benchmark_harness.py \
        "ai/benchmarks/tasks/{{task}}.yaml" \
        --root . \
        --harness {{harness}} \
        --out-dir "$out"
    echo ""
    echo "Result: $out/"
    ls -la "$out/"*.json 2>/dev/null

# Build release binary
build:
    {{cargo}} build --release -p omegon

# Install/update the stable launcher and register this checkout as an Omegon channel.
# Usage: just link [channel]  (default channel: default)
link channel="default":
    #!/usr/bin/env bash
    set -euo pipefail
    # Reconcile jj+git colocated state before HEAD checks.
    ./scripts/sync-jj-to-git.sh
    if ! git symbolic-ref -q HEAD >/dev/null 2>&1 && [ "${OMEGON_ALLOW_DETACHED_LINK:-0}" != "1" ]; then
        echo "✗ Detached HEAD. Refusing to link from an unattached commit."
        echo "  Check out main (or set OMEGON_ALLOW_DETACHED_LINK=1 for an intentional tagged/worktree build)."
        exit 1
    fi
    echo "── Building release binary for current HEAD ──"
    {{cargo}} build --release -p omegon
    BINARY="$(pwd)/target/release/omegon"
    if [ ! -x "$BINARY" ]; then
        echo "No release binary found at $BINARY"
        exit 1
    fi

    mkdir -p "$HOME/.local/bin" "$HOME/.omegon/channels" "$HOME/.omegon/bin"
    install -m 0755 scripts/omegon-launcher.sh "$HOME/.local/bin/omegon"
    install -m 0755 scripts/omegon-launcher.sh "$HOME/.local/bin/om"
    printf '%s\n' "$(pwd)" > "$HOME/.omegon/channels/{{channel}}"

    # Keep a stable fallback copy for invocations outside any checkout/channel.
    install -m 0755 "$BINARY" "$HOME/.omegon/bin/omegon"

    # Leave a compatibility snippet, but the PATH launcher is now canonical.
    ALIAS_FILE="$HOME/.omegon/dev-alias.sh"
    printf '# Generated by just link. Omegon now uses ~/.local/bin/omegon as a stable launcher.\n# Optional explicit override for this checkout:\n# export OMEGON_DEV_ROOT='\''%s'\''\n' \
        "$(pwd)" > "$ALIAS_FILE"

    echo "✓ launcher → $HOME/.local/bin/omegon"
    echo "✓ launcher → $HOME/.local/bin/om"
    echo "✓ channel {{channel}} → $(pwd)"
    echo "✓ fallback → $HOME/.omegon/bin/omegon"
    "$HOME/.local/bin/omegon" --which
    just install-skills
    just install-catalog


# Diagnose Omegon launcher/channel resolution for this machine.
link-doctor:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "PATH entries for omegon:"
    which -a omegon 2>/dev/null || true
    echo ""
    echo "Shell resolution:"
    type -a omegon 2>/dev/null || true
    echo ""
    echo "Launcher diagnosis:"
    if command -v omegon >/dev/null 2>&1; then
        omegon --which || true
    else
        echo "omegon not found on PATH"
    fi
    echo ""
    echo "Channels:"
    if [ -d "$HOME/.omegon/channels" ]; then
        for channel in "$HOME/.omegon/channels"/*; do
            [ -e "$channel" ] || continue
            echo "  $(basename "$channel") -> $(cat "$channel")"
        done
    else
        echo "  none ($HOME/.omegon/channels missing)"
    fi
    echo ""
    echo "Fallback binary:"
    if [ -x "$HOME/.omegon/bin/omegon" ]; then
        echo "  $HOME/.omegon/bin/omegon"
        printf '  '
        "$HOME/.omegon/bin/omegon" --version || true
    else
        echo "  missing: $HOME/.omegon/bin/omegon"
    fi



# Build the full Omegon OCI substrate for the local Linux architecture.
# Defaults to aarch64-linux on Apple Silicon and x86_64-linux on x86_64 hosts.
oci-build-local image="oci-full":
    #!/usr/bin/env bash
    set -euo pipefail
    system="${OCI_SYSTEM:-}"
    if [ -z "$system" ]; then
        case "$(uname -m)" in
            arm64|aarch64) system="aarch64-linux" ;;
            x86_64|amd64) system="x86_64-linux" ;;
            *) echo "✗ Cannot infer OCI_SYSTEM from $(uname -m); set OCI_SYSTEM=aarch64-linux or x86_64-linux"; exit 1 ;;
        esac
    fi
    echo "── Building {{image}} for $system ──"
    nix build ".#{{image}}" --accept-flake-config --system "$system" -o "result-{{image}}-$system"
    echo "✓ result-{{image}}-$system"
    echo "  If this host is not trusted for --system, configure a Linux builder for $system and rerun."

# Export a nix2container image result to a Docker archive for local runtime loading.
# Defaults to the local build symlink produced by `just oci-build-local`.
oci-export-local image="oci-full" tag="ghcr.io/styrene-lab/omegon-full:0.27.0-local":
    #!/usr/bin/env bash
    set -euo pipefail
    system="${OCI_SYSTEM:-}"
    if [ -z "$system" ]; then
        case "$(uname -m)" in
            arm64|aarch64) system="aarch64-linux" ;;
            x86_64|amd64) system="x86_64-linux" ;;
            *) echo "✗ Cannot infer OCI_SYSTEM from $(uname -m); set OCI_SYSTEM=aarch64-linux or x86_64-linux"; exit 1 ;;
        esac
    fi
    archive="${OCI_ARCHIVE:-result-{{image}}-$system.tar}"
    echo "── Exporting {{image}} for $system to $archive ──"
    nix build ".#{{image}}.copyTo" --accept-flake-config -o "result-{{image}}-copy"
    "result-{{image}}-copy/bin/copy-to" "docker-archive:$archive:{{tag}}"
    echo "✓ $archive"

# Load a Docker archive produced by `just oci-export-local` into Podman or Docker.
oci-load-local archive="result-oci-full-aarch64-linux.tar":
    #!/usr/bin/env bash
    set -euo pipefail
    runtime="${OCI_RUNTIME:-podman}"
    if ! command -v "$runtime" >/dev/null 2>&1; then
        echo "✗ OCI runtime not found: $runtime"
        echo "  Install podman, or run with OCI_RUNTIME=docker if Docker is available."
        exit 1
    fi
    "$runtime" load -i "{{archive}}"

# Smoke-test an Omegon OCI image as an explicit subagent substrate.
# Podman is canonical; set OCI_RUNTIME=docker for Docker-compatible hosts.
oci-smoke image="ghcr.io/styrene-lab/omegon-full":
    #!/usr/bin/env bash
    set -euo pipefail
    runtime="${OCI_RUNTIME:-podman}"
    if ! command -v "$runtime" >/dev/null 2>&1; then
        echo "✗ OCI runtime not found: $runtime"
        echo "  Install podman, or run with OCI_RUNTIME=docker if Docker is available."
        exit 1
    fi
    omegon_mount="ro"
    if [ "${OCI_OMEGON_HOME_RW:-0}" = "1" ]; then
        omegon_mount="rw"
    fi
    workspace_opts=":Z"
    omegon_home_opts=":${omegon_mount},Z"
    platform_args=()
    if [ "$runtime" = "docker" ]; then
        workspace_opts=""
        omegon_home_opts=":${omegon_mount}"
    fi
    if [ -n "${OCI_PLATFORM:-}" ]; then
        platform_args=(--platform "$OCI_PLATFORM")
    fi
    "$runtime" run --rm "${platform_args[@]}" \
        -v "$(pwd):/workspace${workspace_opts}" \
        -v "$HOME/.omegon:/data/omegon${omegon_home_opts}" \
        -w /workspace \
        "{{image}}" \
        bash -lc 'omegon --version && git --version && just --version && rg --version && jq --version && python --version && node --version && rustc --version && cargo --version && kubectl version --client=true && helm version --short'

# Install bundled skills to ~/.omegon/skills/ so they are available to all projects.
# Uses the binary itself (embedded assets) so this works for both source and brew installs.
# Project-local skills go in .omegon/skills/ inside each repo.
install-skills:
    #!/usr/bin/env bash
    set -euo pipefail
    # Prefer the release binary; fall back to rsync from source if binary not yet built.
    BINARY=""
    for candidate in "$(pwd)/target/release/omegon" "$(pwd)/target/dev-release/omegon" "$(command -v omegon 2>/dev/null || true)"; do
        if [ -f "$candidate" ] && [ -x "$candidate" ]; then
            BINARY="$candidate"
            break
        fi
    done
    if [ -n "$BINARY" ]; then
        "$BINARY" skills install
    else
        # Binary not built yet — fall back to rsync from source tree
        SKILLS_SRC="$(pwd)/skills"
        SKILLS_DEST="$HOME/.omegon/skills"
        if [ ! -d "$SKILLS_SRC" ]; then
            echo "  no skills/ directory found — skipping"
            exit 0
        fi
        mkdir -p "$SKILLS_DEST"
        rsync -a --delete "$SKILLS_SRC/" "$SKILLS_DEST/"
        count=$(find "$SKILLS_DEST" -name "SKILL.md" | wc -l | tr -d ' ')
        echo "✓ $count skill(s) → $SKILLS_DEST  (rsync fallback — build binary for embedded install)"
    fi

# Install bundled agents to ~/.omegon/catalog/ so they are available to all projects.
# Uses the binary itself (embedded assets) so this works for both source and brew installs.
# Project-local agents go in .omegon/catalog/ inside each repo.
install-catalog:
    #!/usr/bin/env bash
    set -euo pipefail
    # Prefer the release binary; fall back to rsync from source if binary not yet built.
    BINARY=""
    for candidate in "$(pwd)/target/release/omegon" "$(pwd)/target/dev-release/omegon" "$(command -v omegon 2>/dev/null || true)"; do
        if [ -f "$candidate" ] && [ -x "$candidate" ]; then
            BINARY="$candidate"
            break
        fi
    done
    if [ -n "$BINARY" ]; then
        "$BINARY" catalog install --offline
    else
        # Binary not built yet — fall back to rsync from source tree
        CATALOG_SRC="$(pwd)/catalog"
        CATALOG_DEST="$HOME/.omegon/catalog"
        if [ ! -d "$CATALOG_SRC" ]; then
            echo "  no catalog/ directory found — skipping"
            exit 0
        fi
        mkdir -p "$CATALOG_DEST"
        rsync -a --delete "$CATALOG_SRC/" "$CATALOG_DEST/"
        count=$(find "$CATALOG_DEST" -name "agent.toml" | wc -l | tr -d ' ')
        echo "✓ $count agent(s) → $CATALOG_DEST  (rsync fallback — build binary for embedded install)"
    fi

# Pull latest and build (handles Cargo.lock conflicts from version bumps)
# Uses dev-release profile: optimized but fast link (~90% perf, ~10% link time)
update:
    git checkout -- Cargo.lock 2>/dev/null || true
    git checkout -- .omegon/history 2>/dev/null || true
    git pull --rebase
    {{cargo}} build --profile dev-release -p omegon
    @echo "Updated to $(./target/dev-release/omegon --version 2>/dev/null || echo 'build failed')"

# Build and link a specific tagged stable release into a durable local worktree.
# This is the supported way to pin the installed CLI to a release tag.
link-tag tag:
    #!/usr/bin/env bash
    set -euo pipefail

    TAG="{{tag}}"
    if [ -z "$TAG" ]; then
        echo "✗ Usage: just link-tag vX.Y.Z"
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
    BINARY="$WT/target/release/omegon"

    mkdir -p "$(pwd)/.omegon/release-worktrees"

    echo "Preparing durable worktree for $TAG..."
    if [ ! -d "$WT/.git" ] && [ ! -f "$WT/.git" ]; then
        echo "  creating worktree at $WT"
        git worktree add --detach "$WT" "$TAG"
    else
        echo "  reusing existing worktree at $WT"
    fi

    MANIFEST_VERSION=$(grep '^version = ' "$WT/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
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
            (cd "$WT" && {{cargo}} build --release -p omegon)
        fi
    else
        echo "Building $TAG in durable worktree..."
        echo "  no existing tagged binary found; building release binary"
        (cd "$WT" && {{cargo}} build --release -p omegon)
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
    {{cargo}} build --release -p omegon

# Run this workspace's dev-release binary directly after rebuilding it from current source
run *args:
    #!/usr/bin/env bash
    set -euo pipefail
    {{cargo}} build --profile dev-release -p omegon
    exec ./target/dev-release/omegon {{args}}

# ─── Release ─────────────────────────────────────────────────

# Create and push the release/X.Y branch for the current stable line, then switch
# the working copy to it. Release branches are internal stabilization/patch
# branches only; normal feature/refactor work stays on main.
branch-release:
    python3 scripts/release_branch.py branch-release

# Merge the current release/X.Y branch forward into main while preserving
# main's version-state files, then switch back to the release branch. Run after
# every release-branch hardening commit and again after tagging a stable patch.
merge-release-forward branch='':
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -n "{{branch}}" ]; then
        python3 scripts/release_branch.py merge-forward "{{branch}}"
    else
        python3 scripts/release_branch.py merge-forward
    fi

# Release preflight: verify repo is releasable BEFORE any version mutation.
# Checks: on main or release/X.Y, clean tree, release line is stable, changelog
# target exists, docs/install versioned examples are placeholders, and packaging
# automation is consistently wired through the release manifest.
# Called automatically by `just release`. Run manually: just preflight
preflight:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "$(git branch --show-current)" = "main" ]; then
        ./scripts/sync-jj-to-git.sh
    fi
    python3 scripts/release_preflight.py

# Cut a stable release: test, commit milestone state if needed, tag, build.
release:
    #!/usr/bin/env bash
    set -euo pipefail

    # Preflight: proves repo is releasable before any mutation.
    # Checks branch, clean tree, tests, docs/install version, CHANGELOG.
    just preflight

    echo "Rust warning gate..."
    RUSTFLAGS="-D warnings" {{cargo}} check -p omegon -q

    CURRENT=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    if echo "$CURRENT" | grep -q '-'; then
        echo "✗ Release version must be stable semver, got $CURRENT"
        exit 1
    fi
    NEW_VERSION="$CURRENT"
    echo "Releasing: $NEW_VERSION"

    # Mark milestone as released
    ./scripts/milestone-update.sh release "$NEW_VERSION"

    # Refresh the lockfile before commit/tag so stable release steps do not
    # rewrite tracked files after the release commit already exists.
    {{cargo}} check -p omegon -q

    git add Cargo.toml Cargo.lock
    git add -f .omegon/milestones.json
    git commit -m "chore(release): ${NEW_VERSION}"
    git tag "v${NEW_VERSION}"

    echo "Building..."
    {{cargo}} build --release -p omegon 2>&1 | tail -3

    echo ""
    echo "✓ ${NEW_VERSION} — tested, committed, tagged, built."
    BRANCH=$(git branch --show-current)
    echo "  To publish: git push origin ${BRANCH} v${NEW_VERSION}"
    echo ""
    echo "Stable release committed and tagged. Run 'just publish' to push and trigger CI."
# Sign the local macOS validation binary with Apple Developer ID (YubiKey).
# Interactive — prompts for PIN and touch.
# This signs the workstation build used for local validation; distributable
# release artifacts are built and signed separately in CI after `just publish`.
# Run after `just release` if SMARTCARD_PIN wasn't set.
sign:
    #!/usr/bin/env bash
    set -euo pipefail
    BINARY="$(pwd)/target/release/omegon"
    if [ ! -f "$BINARY" ]; then
        echo "✗ No binary at $BINARY — run 'just release' first."
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
# Prevents macOS keychain permission prompts on local release builds.
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
        echo "✓ Signing identity created. Future local release builds can be signed."
        security find-identity -v -p codesigning | grep "Omegon"
    else
        echo "⚠ Certificate imported but not showing as valid signing identity."
        echo "  Open Keychain Access → Certificates → Omegon Local Dev"
        echo "  → Get Info → Trust → Code Signing → Always Trust"
    fi

# Publish: push refs, trigger CI release/site workflows, build docs locally,
# link the local binary, and run a smoke test.
# Optional local YubiKey signing (`just sign`) is for workstation validation.
# Downstream package surfaces consume CI-built release artifacts, not the local binary.
# Flow: just release → just sign (optional) → just publish
publish:
    #!/usr/bin/env bash
    set -euo pipefail

    BINARY="$(pwd)/target/release/omegon"

    # Read version from the built binary — not Cargo.toml.
    # The binary is the release artifact that was built and signed locally.
    if [ ! -f "$BINARY" ]; then
        echo "✗ No binary at $BINARY — run 'just release' first."
        exit 1
    fi
    VERSION=$("$BINARY" --version 2>/dev/null | awk '{print $2}' | head -1)
    if [ -z "$VERSION" ]; then
        echo "✗ Could not read version from binary. Rebuild with: just release"
        exit 1
    fi
    TAG="v${VERSION}"

    echo "╭──────────────────────────────────╮"
    echo "│  Publishing omegon ${VERSION}    │"
    echo "╰──────────────────────────────────╯"

    # ── 1. Pre-flight checks ──────────────────────────────────
    if [ -n "$(git status --porcelain)" ]; then
        echo "✗ Uncommitted changes. Commit or stash first."
        exit 1
    fi

    # Verify tag exists
    if ! git tag --list "$TAG" | grep -q "$TAG"; then
        echo "✗ Tag $TAG not found. Run 'just release' first."
        exit 1
    fi

    # Verify binary version matches
    BINARY_VERSION=$("$BINARY" --version 2>/dev/null | head -1 || echo "unknown")
    echo "  Binary:  $BINARY_VERSION"

    # Check signing status
    SIGN_STATUS=$(python3 scripts/release_status.py --binary "$BINARY")
    echo "  Signing: $SIGN_STATUS"

    # ── 2. Push to origin (triggers CI: release, npm, site) ──
    echo ""
    echo "Pushing to origin..."
    BRANCH=$(git branch --show-current)
    if [ -z "$BRANCH" ]; then
        echo "✗ Detached HEAD. Check out main or release/X.Y before publishing."
        exit 1
    fi

    # Push current release branch + the specific stable tag only.
    # Do NOT use --tags: pushing many accumulated tags at once causes GitHub
    # Actions to silently drop workflow triggers beyond ~3 ref changes per push.
    git push origin "$BRANCH" "$TAG"

    echo ""
    echo "CI workflows triggered:"
    echo "  • release.yml  → GitHub Release with cosign-signed binaries"
    echo "  • site.yml     → omegon.styrene.io (stable) + omegon.styrene.dev (preview) docs rebuild"

    # ── 3. Build docs site locally (verification) ─────────────
    echo ""
    echo "Building docs site locally..."
    pushd site >/dev/null
    node scripts/build-design-tree.mjs 2>/dev/null
    npx astro build 2>&1 | tail -5
    PAGES=$(find dist -name '*.html' | wc -l | tr -d ' ')
    echo "  Pages: $PAGES"
    popd >/dev/null

    # ── 4. Link the binary ────────────────────────────────────
    # Release publishing installs the exact release artifact directly. Source
    # checkout development uses `just link`, which installs the stable launcher
    # and channel metadata instead of package-manager/release paths.
    echo ""
    if [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
        DEST="/usr/local/bin/omegon"
        ALT="$HOME/.local/bin/omegon"
    else
        mkdir -p "$HOME/.local/bin"
        DEST="$HOME/.local/bin/omegon"
        ALT="/usr/local/bin/omegon"
    fi
    rm -f "$ALT" 2>/dev/null || true
    ln -sf "$BINARY" "$DEST"
    echo "✓ omegon → $DEST"
    echo "  hash -d omegon 2>/dev/null || true   (if your shell cached the old path)"

    # ── 5. Run post-publish smoke test ────────────────────────
    echo ""
    just smoke

    # ── 6. Summary ────────────────────────────────────────────
    echo ""
    echo "╭──────────────────────────────────╮"
    echo "│  ✓ Published ${VERSION}          │"
    echo "╰──────────────────────────────────╯"
    echo ""
    echo "  Binary:   $(which omegon) → $BINARY"
    echo "  Signing:  $SIGN_STATUS"
    echo "  Docs:     $PAGES pages built → CI deploying to omegon.styrene.io (stable) and omegon.styrene.dev (preview)"
    echo "  Packages: downstream packaging automation runs from published release artifacts"
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
    {{cargo}} zigbuild --release --target x86_64-unknown-linux-gnu -p omegon
    @ls -lh target/x86_64-unknown-linux-gnu/release/omegon
    @file target/x86_64-unknown-linux-gnu/release/omegon

# Build for Linux aarch64 (via zig cross-linker)
build-linux-arm64:
    {{cargo}} zigbuild --release --target aarch64-unknown-linux-gnu -p omegon
    @ls -lh target/aarch64-unknown-linux-gnu/release/omegon
    @file target/aarch64-unknown-linux-gnu/release/omegon

# Build all release targets (macOS native + Linux via zig)
build-all: build build-linux-amd64 build-linux-arm64
    @echo ""
    @echo "Built:"
    @ls -lh target/release/omegon
    @ls -lh target/x86_64-unknown-linux-gnu/release/omegon
    @ls -lh target/aarch64-unknown-linux-gnu/release/omegon

# Package release archives for all targets
package:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
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
    if [ -f target/release/omegon ]; then
        package_target "aarch64-apple-darwin" "target/release/omegon"
    fi

    # Linux x86_64
    if [ -f target/x86_64-unknown-linux-gnu/release/omegon ]; then
        package_target "x86_64-unknown-linux-gnu" "target/x86_64-unknown-linux-gnu/release/omegon"
    fi

    # Linux aarch64
    if [ -f target/aarch64-unknown-linux-gnu/release/omegon ]; then
        package_target "aarch64-unknown-linux-gnu" "target/aarch64-unknown-linux-gnu/release/omegon"
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
    (cd "$WORKTREE" && {{cargo}} build --release -p omegon)
    BINARY="$WORKTREE/target/release/omegon"

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
    #!/usr/bin/env bash
    set -euo pipefail
    if [ ! -d ../omegon-pi ]; then
        echo "Skipping TypeScript tests: ../omegon-pi is not present."
        exit 0
    fi
    cd ../omegon-pi && npx tsx --test tests/*.test.ts extensions/**/*.test.ts

# TS type check
typecheck:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ ! -d ../omegon-pi ]; then
        echo "Skipping TypeScript typecheck: ../omegon-pi is not present."
        exit 0
    fi
    cd ../omegon-pi && npx tsc --noEmit

# Full TS check: typecheck + lifecycle + tests
check-ts:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ ! -d ../omegon-pi ]; then
        echo "Skipping TypeScript check: ../omegon-pi is not present."
        exit 0
    fi
    cd ../omegon-pi && npm run check

# ─── Armory (omegon-armory) ─────────────────────────────────

# Run armory plugin tests
test-armory:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ ! -d /tmp/omegon-armory ]; then
        echo "Skipping armory tests: /tmp/omegon-armory is not present."
        exit 0
    fi
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
test-all: test-rust

# Quick pre-commit check: local Rust workspace only
pre-commit: check

# Local CI-equivalent for this repository
ci: lint test-rust schema-check

# ─── Counts ─────────────────────────────────────────────────

# Show test counts across all suites
test-count:
    #!/usr/bin/env bash
    set -e
    echo "=== Rust ==="
    {{cargo}} test --workspace 2>&1 | grep "test result" | awk '{s+=$4; f+=$6} END {printf "  %d passed, %d failed\n", s, f}'
    echo "=== TypeScript ==="
    if [ -d ../omegon-pi ]; then cd ../omegon-pi && npx tsx --test tests/*.test.ts extensions/**/*.test.ts 2>&1 | grep "^ℹ tests" | awk '{printf "  %s tests\n", $3}'; else echo "  (../omegon-pi not present)"; fi
    echo "=== Armory ==="
    if [ -d /tmp/omegon-armory ]; then cd /tmp/omegon-armory && npx tsx --test tests/*.test.ts 2>&1 | grep "^ℹ tests" | awk '{printf "  %s tests\n", $3}'; else echo "  (/tmp/omegon-armory not present)"; fi

# ─── Memory schema ──────────────────────────────────────────

# Regenerate the schema contract file after schema changes
schema-regen:
    {{cargo}} test -p omegon-memory schema_contract_generate -- --ignored

# Verify schema contract is current
schema-check:
    {{cargo}} test -p omegon-memory schema_contract_is_current

# ─── Secrets ────────────────────────────────────────────────

# Run secrets crate tests
test-secrets:
    {{cargo}} test -p omegon-secrets

# ─── MCP ────────────────────────────────────────────────────

# Run MCP transport tests
test-mcp:
    {{cargo}} test -p omegon plugins::mcp

# ─── Plugins ────────────────────────────────────────────────

# Run all plugin tests (armory + mcp + registry)
test-plugins:
    {{cargo}} test -p omegon plugins

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
    {{cargo}} cyclonedx --format json --output-cdx
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

    # 1. Binary works — check the release binary directly, NOT cargo run.
    # cargo run recompiles against the current Cargo.toml version and can
    # produce a different version string from the built release artifact.
    RELEASE_BIN="$(pwd)/target/release/omegon"
    if [ ! -f "$RELEASE_BIN" ]; then
        echo "  ✗ No release binary at $RELEASE_BIN — run 'just release' first"
        FAIL=1
        VERSION="MISSING"
    else
        VERSION=$("$RELEASE_BIN" --version 2>/dev/null || echo "FAILED")
    fi
    echo "  Binary: $VERSION"
    [[ "$VERSION" == *"omegon"* ]] || { echo "  ✗ Binary doesn't produce version"; FAIL=1; }

    # 2. Test count doesn't drop below known floor
    TEST_COUNT=$({{cargo}} test -p omegon 2>&1 | awk '/test result: ok/ { sum += $4 } END { print sum + 0 }')
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
