---
name: git
description: Git conventions — conventional commits, semantic versioning, branch naming, tagging, changelogs, and release workflow. Use when committing, branching, tagging, or managing releases.
---

# Git Skill

Conventions for git operations. Covers commit messages, versioning, branching, tagging, and changelogs.

## Conventional Commits

Use [Conventional Commits](https://www.conventionalcommits.org/) for all commit messages.

### Format

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

### Types

| Type | When | Semver Impact |
|------|------|---------------|
| `feat` | New feature or capability | MINOR bump |
| `fix` | Bug fix | PATCH bump |
| `docs` | Documentation only | none |
| `style` | Formatting, whitespace (no logic change) | none |
| `refactor` | Code change that neither fixes nor adds | none |
| `perf` | Performance improvement | none |
| `test` | Adding or correcting tests | none |
| `chore` | Build process, deps, tooling, version bumps | none |
| `ci` | CI/CD configuration changes | none |
| `revert` | Reverts a previous commit | varies |

### Scope (optional)

Parenthetical hint narrowing what changed. Free-form but consistent within a repo:

```
feat(auth): add OAuth2 PKCE flow
fix(db): handle connection pool exhaustion
test(api): add rate limit validation tests
chore(deps): bump serde to 1.0.200
```

### Breaking Changes

Mark with `!` after the type/scope, and explain in the footer:

```
feat(api)!: change response envelope format

BREAKING CHANGE: Response wrapper changed from { data } to { result }.
Clients must update to SDK >= 2.0.0.
```

### Commit Message Quality

**Good** — explains the *why*, not just the *what*:
```
fix: normalize truncated hashes in message routing

Short hashes from legacy clients caused lookup failures in the store.
Now pads to full length before resolution.
```

**Bad** — restates the diff:
```
fix: change hash lookup code
```

### Validation Regex

```
^(feat|fix|docs|style|refactor|perf|test|chore|ci|revert)(\(.+\))?(!)?: .+
```

## Semantic Versioning

Follow [SemVer 2.0.0](https://semver.org/):

```
vMAJOR.MINOR.PATCH
```

| Component | Increment When |
|-----------|---------------|
| **MAJOR** | Breaking API/protocol change |
| **MINOR** | New feature, backward-compatible |
| **PATCH** | Bug fix, backward-compatible |

### Pre-1.0 Convention

During `0.x.y` development:
- MINOR bumps may include breaking changes
- PATCH bumps are bug fixes
- API stability is not guaranteed until `1.0.0`

### Version Bump Commits

Use `chore` type:
```
chore: bump version to 0.4.0
```

## Branch Naming

```
<type>/<short-description>
```

| Type | Purpose |
|------|---------|
| `feature/` | New functionality |
| `fix/` | Bug fix |
| `patch/` | Small targeted fix |
| `chore/` | Tooling, deps, config |
| `refactor/` | Code restructuring |
| `perf/` | Performance work |
| `breaking/` | Known breaking change |
| `hotfix/` | Urgent production fix |

**Examples:**
```
feature/oauth-integration
fix/connection-pool-leak
chore/bump-dependencies
refactor/split-service-layer
```

**Main branch**: `main`. Tags and releases are cut from main only.

## Tagging

```bash
git tag v0.4.0
git push origin v0.4.0
```

- Always prefix with `v`: `v0.4.0`, not `0.4.0`
- Tags should be on the `main` branch
- Format: `vMAJOR.MINOR.PATCH`
- Tag triggers release CI (build, publish, GitHub Release)

### Release Flow

```
1. Complete work on feature/fix branch
2. Merge to main via PR
3. Update version in source files
4. Commit: chore: bump version to X.Y.Z
5. Update CHANGELOG.md
6. Tag: git tag vX.Y.Z
7. Push: git push origin main --tags
8. CI builds and publishes
```

## Changelog

Maintain `CHANGELOG.md` using [Keep a Changelog](https://keepachangelog.com/) format:

```markdown
# Changelog

## [Unreleased]

## [0.4.0] - 2026-02-03

### Added
- New feature description

### Changed
- Modified behavior description

### Fixed
- Bug fix description
```

### Section Order

`Added` → `Changed` → `Deprecated` → `Removed` → `Fixed` → `Security`

Only include sections that have entries. `[Unreleased]` collects changes since the last tag.

## Quick Reference

```bash
# Feature branch
git checkout -b feature/my-feature main
git commit -m "feat(scope): add new capability"
git push -u origin feature/my-feature

# Fix branch
git checkout -b fix/the-bug main
git commit -m "fix: resolve crash on empty input"

# Release (from main)
git checkout main && git pull
# bump version, update CHANGELOG
git commit -m "chore: bump version to 0.5.0"
git tag v0.5.0
git push origin main --tags
```


