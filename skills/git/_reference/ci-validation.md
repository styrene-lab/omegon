+++
id = "5fd0ef05-d85c-4de3-a317-804cc461b6df"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Git CI Validation Templates

Workflow templates for enforcing git conventions in CI. Based on patterns from cleave.

## Branch Name Validation

```yaml
validate-branch:
  name: Validate Branch Name
  runs-on: ubuntu-latest
  if: github.event_name == 'pull_request'
  steps:
    - name: Check branch naming convention
      run: |
        BRANCH="${{ github.head_ref }}"
        if [[ ! "$BRANCH" =~ ^(feature|fix|patch|chore|refactor|perf|breaking|hotfix)/.+ ]]; then
          echo "ERROR: Branch name must follow convention: <type>/<description>"
          echo "   Valid types: feature, fix, patch, chore, refactor, perf, breaking, hotfix"
          echo "   Your branch: $BRANCH"
          exit 1
        fi
        echo "Branch name follows convention: $BRANCH"
```

## Conventional Commit Validation

```yaml
validate-commits:
  name: Validate Commit Messages
  runs-on: ubuntu-latest
  if: github.event_name == 'pull_request'
  steps:
    - name: Checkout code
      uses: actions/checkout@v4
      with:
        fetch-depth: 0

    - name: Validate conventional commits
      run: |
        COMMITS=$(git log --format=%s origin/main..HEAD)

        PATTERN="^(feat|fix|docs|style|refactor|perf|test|chore|ci|revert)(\(.+\))?(!)?: .+"

        FAILED=0
        while IFS= read -r commit; do
          if [[ ! "$commit" =~ $PATTERN ]]; then
            echo "FAIL: $commit"
            FAILED=1
          else
            echo "OK: $commit"
          fi
        done <<< "$COMMITS"

        if [ $FAILED -eq 1 ]; then
          echo ""
          echo "Commit messages must follow Conventional Commits:"
          echo "  <type>(<scope>): <description>"
          echo ""
          echo "Types: feat, fix, docs, style, refactor, perf, test, chore, ci, revert"
          exit 1
        fi
```

## Tag Validation (Release Workflow)

```yaml
name: Release
on:
  push:
    tags:
      - 'v*.*.*'

jobs:
  validate-tag:
    name: Validate Tag
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Verify tag is on main branch
        run: |
          TAG_COMMIT=$(git rev-list -n 1 ${{ github.ref }})
          MAIN_COMMITS=$(git rev-list origin/main)

          if ! echo "$MAIN_COMMITS" | grep -q "$TAG_COMMIT"; then
            echo "ERROR: Tag ${{ github.ref_name }} is not on main branch"
            echo "   Tags must only be created from main branch"
            exit 1
          fi
          echo "Tag ${{ github.ref_name }} is on main branch"

      - name: Validate semantic version format
        run: |
          if [[ ! "${{ github.ref_name }}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            echo "ERROR: Tag must follow semantic versioning: vMAJOR.MINOR.PATCH"
            echo "   Got: ${{ github.ref_name }}"
            exit 1
          fi
          echo "Valid semantic version: ${{ github.ref_name }}"
```

## Push Triggers with Branch Naming

Use branch patterns in push triggers to only run CI on convention-following branches:

```yaml
on:
  push:
    branches:
      - main
      - 'feature/**'
      - 'fix/**'
      - 'patch/**'
      - 'chore/**'
      - 'refactor/**'
      - 'perf/**'
      - 'breaking/**'
  pull_request:
    branches:
      - main
```

## Combined CI Template

Full CI workflow with branch, commit, lint, and test validation:

```yaml
name: CI
on:
  push:
    branches:
      - main
      - 'feature/**'
      - 'fix/**'
      - 'chore/**'
  pull_request:
    branches:
      - main

jobs:
  validate-branch:
    name: Validate Branch Name
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'
    steps:
      - name: Check branch naming convention
        run: |
          BRANCH="${{ github.head_ref }}"
          if [[ ! "$BRANCH" =~ ^(feature|fix|patch|chore|refactor|perf|breaking|hotfix)/.+ ]]; then
            echo "ERROR: Branch name must follow convention: <type>/<description>"
            exit 1
          fi

  validate-commits:
    name: Validate Commit Messages
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Validate conventional commits
        run: |
          PATTERN="^(feat|fix|docs|style|refactor|perf|test|chore|ci|revert)(\(.+\))?(!)?: .+"
          FAILED=0
          while IFS= read -r commit; do
            [[ "$commit" =~ $PATTERN ]] || { echo "FAIL: $commit"; FAILED=1; }
          done < <(git log --format=%s origin/main..HEAD)
          [ $FAILED -eq 0 ] || exit 1

  test:
    name: Test
    runs-on: ubuntu-latest
    needs: [validate-branch, validate-commits]
    if: always() && !failure()
    steps:
      - uses: actions/checkout@v4
      # ... language-specific test steps
```

## Pre-commit Hook (Local Enforcement)

Optional local validation via git hooks. Add to `.git/hooks/commit-msg`:

```bash
#!/usr/bin/env bash
# Validate conventional commit format
PATTERN="^(feat|fix|docs|style|refactor|perf|test|chore|ci|revert)(\(.+\))?(!)?: .+"
MSG=$(head -1 "$1")

if [[ ! "$MSG" =~ $PATTERN ]]; then
    echo "ERROR: Commit message does not follow Conventional Commits"
    echo "  Expected: <type>(<scope>): <description>"
    echo "  Got:      $MSG"
    echo ""
    echo "  Types: feat, fix, docs, style, refactor, perf, test, chore, ci, revert"
    exit 1
fi
```

Make executable: `chmod +x .git/hooks/commit-msg`
