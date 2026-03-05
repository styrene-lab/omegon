---
name: oci
description: OCI container and artifact best practices. Covers Containerfile authoring, multi-arch builds, registry authentication, image tagging, security scanning, and artifact management. Use when building, pushing, or managing container images and OCI artifacts.
---

# OCI Skill

Conventions for container images, OCI artifacts, and registry operations.

## Core Conventions

- **Containerfile** (not Dockerfile) as the canonical name
- **Podman** preferred, Docker compatible — commands are interchangeable
- **Multi-stage builds** to minimize final image size
- **Non-root** user in production images
- **Immutable tags** — never overwrite a published tag

## Registry Authentication

### GHCR (GitHub Container Registry)

```bash
echo "$GITHUB_TOKEN" | docker login ghcr.io -u USERNAME --password-stdin
```

### AWS ECR

```bash
aws ecr get-login-password --region us-east-1 | \
  docker login --username AWS --password-stdin ACCOUNT.dkr.ecr.us-east-1.amazonaws.com
```

ECR tokens expire after 12 hours. In CI, call before every push.

### Docker Hub

```bash
echo "$DOCKER_TOKEN" | docker login -u USERNAME --password-stdin
```

### Credential Helpers

| Registry | Helper |
|----------|--------|
| GHCR | `docker-credential-gh` (via `gh auth setup-docker`) |
| ECR | `docker-credential-ecr-login` (from `amazon-ecr-credential-helper`) |
| GCR/GAR | `docker-credential-gcloud` |

Configure in `~/.docker/config.json`:
```json
{
  "credHelpers": {
    "ghcr.io": "gh",
    "ACCOUNT.dkr.ecr.REGION.amazonaws.com": "ecr-login"
  }
}
```

## Containerfile Conventions

### Structure

```dockerfile
# syntax=docker/dockerfile:1
FROM python:3.12-slim AS builder

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY src/ src/

FROM python:3.12-slim
WORKDIR /app
COPY --from=builder /app /app

RUN useradd -r -s /bin/false appuser
USER appuser

EXPOSE 8080
ENTRYPOINT ["python", "-m", "myapp"]
```

### Rules

| Rule | Rationale |
|------|-----------|
| Pin base image digests in CI | Reproducible builds |
| `--no-cache-dir` on pip install | Smaller layers |
| `COPY` specific paths, not `.` | Cache efficiency, avoid leaking secrets |
| `ARG` defaults for versions | Co-locate version with build definition |
| `LABEL org.opencontainers.image.*` | OCI metadata standard |
| One `RUN` per logical step | Readability over layer count (BuildKit caches well) |

### ARG Defaults — Not Build Scripts

Define version defaults in the Containerfile, not in justfiles or Makefiles:

```dockerfile
# Good — version lives with the build definition
ARG PYTHON_VERSION=3.12
FROM python:${PYTHON_VERSION}-slim

# Bad — hardcoded in justfile creates manual update obligation
# just build --build-arg PYTHON_VERSION=3.12
```

Override only when needed: `docker build --build-arg PYTHON_VERSION=3.13 .`

### .dockerignore

Always include one. Minimum:

```
.git
.venv
__pycache__
*.pyc
node_modules
.env
*.secret
```

## Cross-Platform Builds

### Apple Silicon → amd64 Clusters

**Always specify `--platform` when the build host differs from the target.**

```bash
# Building on arm64 Mac for amd64 Kubernetes
docker build --platform linux/amd64 -t myimage:latest .
```

Without `--platform`, the image is arm64 and **fails silently** on amd64 nodes.

### Multi-Arch with Buildx

```bash
docker buildx create --name multiarch --use
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --tag ghcr.io/org/image:v1.0.0 \
  --push .
```

### Cache Busting

Combine `--no-cache` with `--platform` to prevent stale cross-arch layer caches:

```bash
docker build --platform linux/amd64 --no-cache -t myimage:latest .
```

Stale pip caches can serve arm64 wheels even when targeting amd64.

## Tagging Strategy

### Convention

```
REGISTRY/ORG/IMAGE:TAG
```

### Tag Types

| Tag | When | Example |
|-----|------|---------|
| Semver | Release | `v1.2.3` |
| SHA | Every build | `sha-a1b2c3d` |
| Branch | Dev builds | `main`, `feature-x` |
| `latest` | **Avoid** | Ambiguous, not reproducible |

### Tagging Commands

```bash
# Tag with semver + SHA
IMAGE=ghcr.io/org/myapp
SHA=$(git rev-parse --short HEAD)
VERSION=v1.2.3

docker tag myapp:latest "$IMAGE:$VERSION"
docker tag myapp:latest "$IMAGE:sha-$SHA"
docker push "$IMAGE:$VERSION"
docker push "$IMAGE:sha-$SHA"
```

## OCI Artifacts (Beyond Images)

OCI registries store more than container images:

| Artifact | Tool | Push Command |
|----------|------|-------------|
| Helm charts | `helm` | `helm push chart.tgz oci://ghcr.io/org/charts` |
| SBOMs | `cosign` | `cosign attach sbom --sbom sbom.spdx IMAGE` |
| Signatures | `cosign` | `cosign sign IMAGE` |
| Attestations | `cosign` | `cosign attest --predicate provenance.json IMAGE` |
| Arbitrary files | `oras` | `oras push ghcr.io/org/artifact:v1 ./file.txt` |

### Helm OCI Push

```bash
helm package ./chart
helm push chart-1.0.0.tgz oci://ghcr.io/org/charts
```

### Image Signing with Cosign

```bash
# Keyless signing (GitHub Actions OIDC)
cosign sign ghcr.io/org/image@sha256:DIGEST

# Key-based
cosign generate-key-pair
cosign sign --key cosign.key ghcr.io/org/image:v1.0.0
cosign verify --key cosign.pub ghcr.io/org/image:v1.0.0
```

## Security Scanning

### Trivy

```bash
trivy image ghcr.io/org/image:v1.0.0           # Scan image
trivy fs .                                       # Scan filesystem/deps
trivy image --severity HIGH,CRITICAL IMAGE       # Filter severity
trivy image --exit-code 1 IMAGE                  # Fail CI on findings
```

### Docker Scout (Docker Desktop)

```bash
docker scout cves IMAGE                          # CVE scan
docker scout quickview IMAGE                     # Summary
docker scout recommendations IMAGE               # Base image suggestions
```

### Grype

```bash
grype IMAGE                                      # Vulnerability scan
grype dir:.                                      # Scan local project
```

## Registry Management

### Lifecycle Policies (ECR)

```json
{
  "rules": [{
    "rulePriority": 1,
    "description": "Expire untagged after 7 days",
    "selection": {
      "tagStatus": "untagged",
      "countType": "sinceImagePushed",
      "countUnit": "days",
      "countNumber": 7
    },
    "action": { "type": "expire" }
  }]
}
```

### GHCR Cleanup

```bash
# List package versions
gh api user/packages/container/IMAGE/versions | jq '.[].metadata.container.tags'

# Delete untagged
gh api --method DELETE user/packages/container/IMAGE/versions/VERSION_ID
```

### Repository Naming

```
ghcr.io/ORG/SERVICE                    # Application image
ghcr.io/ORG/charts/SERVICE             # Helm chart
ghcr.io/ORG/base/RUNTIME              # Shared base images
```

## CI/CD Integration

### GitHub Actions

```yaml
- uses: docker/login-action@v3
  with:
    registry: ghcr.io
    username: ${{ github.actor }}
    password: ${{ secrets.GITHUB_TOKEN }}

- uses: docker/build-push-action@v6
  with:
    context: .
    push: true
    platforms: linux/amd64,linux/arm64
    tags: |
      ghcr.io/${{ github.repository }}:${{ github.sha }}
      ghcr.io/${{ github.repository }}:latest
    cache-from: type=gha
    cache-to: type=gha,mode=max
```

### Build Cache (BuildKit)

| Cache Backend | Use Case |
|--------------|----------|
| `type=gha` | GitHub Actions (free, limited) |
| `type=registry` | Push cache layers to registry |
| `type=local` | Local directory (CI runners with persistent storage) |

## Debugging

```bash
docker history IMAGE                    # Layer sizes
docker inspect IMAGE                    # Full metadata
docker manifest inspect IMAGE           # Multi-arch manifest
dive IMAGE                              # Interactive layer explorer
crane manifest IMAGE                    # OCI manifest (no Docker needed)
skopeo inspect docker://IMAGE           # Remote inspection
```

## Common Gotchas

| Issue | Fix |
|-------|-----|
| Image works locally, fails in k8s | Missing `--platform linux/amd64` on Apple Silicon build |
| Push denied to GHCR | `gh auth token` scope needs `write:packages` |
| Stale layers after dep update | `--no-cache` or `--no-cache-filter=stage_name` |
| ECR login expired | Re-run `aws ecr get-login-password` (12h TTL) |
| `latest` tag not updating | Tag is a pointer, not auto-updated — push explicitly |
| Large image size | Multi-stage build, slim/distroless base, `.dockerignore` |
| Build context too large | Check `.dockerignore`, avoid `COPY . .` |
| Helm OCI push fails | `helm registry login` first, chart must be packaged `.tgz` |
| Runtime DB lost on restart | Mount persistent volume, or use config-file-backed state |
| `ARG` before `FROM` not visible after | Re-declare `ARG` after `FROM` to use in later stages |
