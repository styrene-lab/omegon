+++
id = "bea103e2-0584-4304-a119-bd518207d2ca"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OCI Registry Login

Authenticate with OCI registries for container image operations.

## 1. Pre-flight Check

```bash
gh auth status
podman login --get-login ghcr.io 2>/dev/null || echo "ghcr.io: not logged in"
aws sts get-caller-identity 2>/dev/null || echo "AWS: not authenticated"
```

## 2. Authenticate

### GitHub Container Registry (ghcr.io)

```bash
# Ensure write:packages scope
gh auth refresh -s write:packages

# Login
gh auth token | podman login ghcr.io -u $(gh api user --jq .login) --password-stdin
```

### AWS ECR

```bash
AWS_ACCOUNT=$(aws sts get-caller-identity --query Account --output text)
AWS_REGION="${AWS_REGION:-${AWS_DEFAULT_REGION:-us-east-1}}"
aws ecr get-login-password --region "$AWS_REGION" | \
  podman login --username AWS --password-stdin "$AWS_ACCOUNT.dkr.ecr.$AWS_REGION.amazonaws.com"
```

### Docker Hub (docker.io)

```bash
podman login docker.io -u <username>
```

## 3. Verify

```bash
podman login --get-login ghcr.io
```

## Troubleshooting

| Problem | Solution |
|---------|----------|
| ghcr.io permission denied | `gh auth refresh -s write:packages` |
| ECR auth failed | `aws sts get-caller-identity` to check session |
| ECR token expired | Re-run login (tokens valid 12 hours) |
| podman not found | Use `docker login` instead |
