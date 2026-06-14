#!/usr/bin/env bash
set -euo pipefail

launcher_path="${BASH_SOURCE[0]}"
launcher_real="$(cd "$(dirname "$launcher_path")" && pwd -P)/$(basename "$launcher_path")"

is_executable_target() {
    local candidate="$1"
    [[ -n "$candidate" && -x "$candidate" ]] || return 1
    local candidate_real
    candidate_real="$(cd "$(dirname "$candidate")" && pwd -P)/$(basename "$candidate")" || return 1
    [[ "$candidate_real" != "$launcher_real" ]]
}

repo_root_from() {
    local dir="$1"
    while [[ "$dir" != "/" ]]; do
        if [[ -f "$dir/Cargo.toml" && -d "$dir/core/crates/omegon" ]]; then
            printf '%s\n' "$dir"
            return 0
        fi
        dir="$(dirname "$dir")"
    done
    return 1
}

target_for_root() {
    local root="$1"
    local rel="$root/target/release/omegon"
    local dev="$root/target/dev-release/omegon"
    if is_executable_target "$rel"; then
        printf '%s\n' "$rel"
        return 0
    fi
    if is_executable_target "$dev"; then
        printf '%s\n' "$dev"
        return 0
    fi
    return 1
}

resolve_target() {
    local target="" root="" channel="${OMEGON_CHANNEL:-default}"

    if [[ -n "${OMEGON_BIN:-}" ]]; then
        if is_executable_target "$OMEGON_BIN"; then
            printf 'env:OMEGON_BIN\t%s\n' "$OMEGON_BIN"
            return 0
        fi
        printf 'omegon launcher: OMEGON_BIN is not executable or points to launcher: %s\n' "$OMEGON_BIN" >&2
        return 1
    fi

    if [[ -n "${OMEGON_DEV_ROOT:-}" ]]; then
        if target="$(target_for_root "$OMEGON_DEV_ROOT")"; then
            printf 'env:OMEGON_DEV_ROOT\t%s\n' "$target"
            return 0
        fi
        printf 'omegon launcher: no runnable binary under OMEGON_DEV_ROOT=%s\n' "$OMEGON_DEV_ROOT" >&2
        return 1
    fi

    if root="$(repo_root_from "$PWD")" && target="$(target_for_root "$root")"; then
        printf 'nearest-checkout\t%s\n' "$target"
        return 0
    fi

    local channel_file="$HOME/.omegon/channels/$channel"
    if [[ -f "$channel_file" ]]; then
        root="$(grep -v '^[[:space:]]*$' "$channel_file" | head -n 1)"
        if target="$(target_for_root "$root")"; then
            printf 'channel:%s\t%s\n' "$channel" "$target"
            return 0
        fi
        printf 'omegon launcher: channel %s has no runnable binary under %s\n' "$channel" "$root" >&2
        return 1
    fi

    target="$HOME/.omegon/bin/omegon"
    if is_executable_target "$target"; then
        printf 'fallback-installed\t%s\n' "$target"
        return 0
    fi

    printf 'omegon launcher: no runnable binary found\n' >&2
    printf 'checked: OMEGON_BIN, OMEGON_DEV_ROOT, nearest checkout, ~/.omegon/channels/%s, ~/.omegon/bin/omegon\n' "$channel" >&2
    printf 'run: just build && just link from an Omegon checkout\n' >&2
    return 127
}

case "${1:-}" in
    --which|which)
        resolved="$(resolve_target)"
        reason="${resolved%%$'\t'*}"
        target="${resolved#*$'\t'}"
        printf 'launcher: %s\n' "$launcher_real"
        printf 'reason: %s\n' "$reason"
        printf 'target: %s\n' "$target"
        if [[ -x "$target" ]]; then
            printf 'version: '
            "$target" --version || true
        fi
        exit 0
        ;;
esac

resolved="$(resolve_target)"
target="${resolved#*$'\t'}"
exec "$target" "$@"
