+++
id = "11fa6470-fa18-4af1-b43f-60d5e45b804f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Version switcher — tfswitch-style binary management for Omegon — Design Spec (extracted)

> Auto-extracted from docs/omegon-version-switcher.md at decide-time.

## Decisions

### Subcommand, interactive picker, auto-detect — full tfswitch feature set (decided)

Subcommand (`omegon switch`) keeps distribution simple during rapid development — no second binary. Interactive picker because it's low incremental cost over the download+symlink core. Auto-detect (`.omegon-version`) because it enables pinning across machines for free. Dev machines keep `just update` for source builds; switcher is for operator machines consuming GitHub Release artifacts.

## Research Summary

### tfswitch design reference

tfswitch (github.com/warrensbox/terraform-switcher) is a Go CLI that manages multiple Terraform binary versions:

**Storage**: `~/.terraform.versions/terraform_0.14.0` — flat directory, one binary per version.

**Switching**: Copies (or symlinks) the selected binary to a PATH location. The active version is whatever binary is at the symlink target.

**Download**: Fetches from HashiCorp's releases page on demand. Checksums verified. Cached — only downloads once per version.

**Auto-detection**: R…

### Omegon version switcher design

**Storage**: `~/.omegon/versions/0.14.1-rc.12/omegon` — directory per version, single binary inside.

**Active version**: `~/.omegon/bin/omegon` symlink pointing into the versions dir. `~/.omegon/bin` is on PATH (install.sh already sets this up, or `/usr/local/bin/omegon` is the symlink target).

**Download source**: GitHub Releases from `styrene-lab/omegon`. RC tags (`v0.14.1-rc.12`) produce pre-release artifacts. Stable tags (`v0.14.1`) produce release artifacts. Same tarballs, same checksums …
