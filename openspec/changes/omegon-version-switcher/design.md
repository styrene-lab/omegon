# Version switcher — tfswitch-style binary management for Omegon — Design

## Architecture Decisions

### Decision: Subcommand, interactive picker, auto-detect — full tfswitch feature set

**Status:** decided
**Rationale:** Subcommand (`omegon switch`) keeps distribution simple during rapid development — no second binary. Interactive picker because it's low incremental cost over the download+symlink core. Auto-detect (`.omegon-version`) because it enables pinning across machines for free. Dev machines keep `just update` for source builds; switcher is for operator machines consuming GitHub Release artifacts.

## Research Context

### tfswitch design reference

tfswitch (github.com/warrensbox/terraform-switcher) is a Go CLI that manages multiple Terraform binary versions:

**Storage**: `~/.terraform.versions/terraform_0.14.0` — flat directory, one binary per version.

**Switching**: Copies (or symlinks) the selected binary to a PATH location. The active version is whatever binary is at the symlink target.

**Download**: Fetches from HashiCorp's releases page on demand. Checksums verified. Cached — only downloads once per version.

**Auto-detection**: Reads `.terraform-version` (single version string) or `.tfswitchrc` from the project root. Also reads `required_version` from Terraform module files. Running `tfswitch` with no args in a project auto-selects.

**CLI**: `tfswitch` (interactive picker), `tfswitch 1.5.0` (exact), `tfswitch --latest-stable 1.5` (latest matching prefix), `tfswitch --latest-pre 0.13` (latest pre-release matching prefix).

**Key insight**: tfswitch doesn't build anything. It downloads pre-built binaries from releases. The version manager is separate from the build system.

### Omegon version switcher design

**Storage**: `~/.omegon/versions/0.14.1-rc.12/omegon` — directory per version, single binary inside.

**Active version**: `~/.omegon/bin/omegon` symlink pointing into the versions dir. `~/.omegon/bin` is on PATH (install.sh already sets this up, or `/usr/local/bin/omegon` is the symlink target).

**Download source**: GitHub Releases from `styrene-lab/omegon`. RC tags (`v0.14.1-rc.12`) produce pre-release artifacts. Stable tags (`v0.14.1`) produce release artifacts. Same tarballs, same checksums — just different release flags.

**CLI surface**:
- `omegon switch` — interactive TUI picker showing installed + available versions
- `omegon switch 0.14.1-rc.12` — install (if needed) and switch to exact version  
- `omegon switch --latest` — switch to latest stable release
- `omegon switch --latest-rc` — switch to latest RC
- `omegon switch --list` — show installed versions, highlight active

**Auto-detection**: `.omegon-version` file in project root. Contains a version string or constraint. When `omegon` starts, if `.omegon-version` exists and the requested version isn't active, it either auto-switches or warns.

**Self-update**: `omegon switch` replaces the current binary. Since the switcher is part of `omegon` itself, any version can switch to any other version. The new binary takes over on next invocation.

**Subcommand, not separate binary**: Unlike tfswitch (standalone Go binary), this is `omegon switch` — a subcommand of the binary itself. The binary knows how to replace itself. No second tool to install or keep updated.

## File Changes

- `core/crates/omegon/src/switch.rs` (new) — Version switcher module — list releases, download, verify checksum, symlink, interactive picker
- `core/crates/omegon/src/main.rs` (modified) — Add Switch subcommand to CLI
- `core/install.sh` (modified) — Align install.sh to use ~/.omegon/versions/ layout so switcher and installer are compatible

## Constraints

- Downloads from GitHub Releases API (styrene-lab/omegon-core)
- Platform detection: aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu
- SHA256 checksum verification from checksums.sha256 artifact
- Symlink target: the path where omegon is currently running (std::env::current_exe)
- Interactive picker: simple terminal list with arrow keys, no ratatui dependency (runs outside TUI)
- .omegon-version auto-detect: read file from cwd ancestors, warn if active version doesn't match
