+++
title = "CI Footprint Consolidation"
tags = ["design","ci","release"]
+++

# CI Footprint Consolidation

---
title: CI Footprint Consolidation
status: deferred
tags: [design, ci, release]
---

# CI Footprint Consolidation

## Overview

The release pipeline currently treats binary release, site publication, Homebrew updates, live smoke validation, ABI validation, and a broad OCI image matrix as one large release-critical surface. The `v0.26.5` release showed the failure mode clearly: the GitHub Release, binary artifacts, checksums, signatures, site, tests, and Homebrew update can all be complete while the parent release workflow remains `in_progress` because long-tail OCI image jobs are still running.

That creates noisy release status, delays operator confidence, and makes patch releases appear unfinished even when the operator-facing artifacts are live.

## Problem Statement

We need to significantly reduce and clarify the CI footprint so each workflow answers one question:

- Is the Rust binary releasable?
- Are release artifacts published?
- Are docs/site updated?
- Is Homebrew updated?
- Are OCI images built and published?
- Are non-blocking smoke/compatibility checks healthy?

Today those concerns are coupled tightly enough that non-essential long-tail jobs obscure the status of essential release work.

## Goals

1. Make patch-release readiness obvious within minutes.
2. Separate release-critical binary publication from optional/slow OCI image publication.
3. Keep full validation available without making every release wait for every artifact class.
4. Preserve signed/checksummed release artifacts and ABI validation.
5. Reduce matrix duplication across binary, browser, OCI, site, and Homebrew workflows.
6. Give operators a single concise release-status answer that does not require reading multiple job trees.

## Non-goals

- Removing OCI images entirely.
- Weakening binary signing/checksum/SBOM guarantees.
- Hiding failed optional jobs; optional means non-blocking, not invisible.
- Rewriting all GitHub Actions at once.

## Current Observations

- `Release Rust binary` includes binary builds, ABI validation, release creation, and OCI image jobs.
- The release can be live while OCI image jobs keep the parent workflow `in_progress`.
- Separate workflows also run for tests, site, and Homebrew updates.
- Release closeout currently needs manual interpretation of workflow/job state.

## Candidate Design

### Workflow classes

#### 1. Required release workflow

Name: `Release binary artifacts`

Responsibilities:

- build target binary matrix
- build browser extension artifacts if they are part of the binary release bundle
- sign/notarize where applicable
- produce checksums/signatures/SBOM/notices
- run ABI validations required for binary compatibility
- create/update GitHub Release

This workflow owns the release-ready signal.

#### 2. Required validation workflow

Name: `Tests`

Responsibilities:

- normal Rust test/lint gates
- deterministic non-network validation
- any release-blocking smoke checks that are fast and reliable

#### 3. Independent publication workflows

Names:

- `Publish site`
- `Update Homebrew formula`

These may be triggered by tag/release completion but should report independently.

#### 4. Optional artifact workflows

Name: `Publish OCI images`

Responsibilities:

- OCI image matrix
- image signing/SBOM/attestations
- registry pushes

This should be independently visible and allowed to continue after the release is live. Its failure should not make the binary release look unfinished.

### Release status contract

A patch release is considered live when:

- tag exists and points to expected commit
- GitHub Release exists and is not draft/prerelease
- required binary artifacts and checksums exist
- required release workflow concludes success
- tests workflow concludes success

OCI images are a separate status dimension:

- `oci: pending|success|failed|skipped`

## Open Questions

- [assumption] OCI images are not required for Flynt/operator patch release consumption.
- Which OCI images are actually consumed today, by whom, and on what cadence?
- Should OCI images trigger from release tags, manually, nightly, or only when container-related files change?
- Which workflow should own SBOM generation for binary vs OCI artifacts?
- Can browser extension packaging be split from binary release, or is it release-critical?
- Should Homebrew update depend on release creation or only tag push?
- Do we need a release-status summarizer command/check that collapses required vs optional workflow state?

## Initial Decisions

- Treat binary release publication and OCI image publication as separate release dimensions.
- Do not let optional OCI matrix duration obscure binary release readiness.
- Preserve OCI visibility through a dedicated workflow rather than deleting it.

## Implementation Sketch

1. Inventory current workflow files and triggers.
2. Classify each job as release-critical, publication, optional artifact, or advisory validation.
3. Split OCI image jobs out of `Release Rust binary` into `Publish OCI images`.
4. Make binary release workflow conclude after release artifacts are created.
5. Add clear workflow names and job summaries with required/optional labels.
6. Add a release-closeout checklist or script that reports:
   - tag
   - release page
   - required assets
   - tests
   - binary release
   - site
   - Homebrew
   - OCI images
7. Reassess runtime/cost after one release.

## Acceptance Criteria

- A future patch release can be declared live without waiting for OCI jobs.
- GitHub Actions UI shows binary release completion separately from OCI image publication.
- A failed OCI image job does not mark the binary release workflow failed.
- Operators can still see OCI publication status explicitly.
- No release-critical artifact/signature/checksum coverage is lost.
