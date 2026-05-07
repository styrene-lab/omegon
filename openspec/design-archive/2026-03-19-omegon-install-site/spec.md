+++
id = "eb0fbc13-1b10-456b-8e03-cc518772bcfb"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# omegon.styrene.dev — installation landing page + install.sh hosting — Design Spec (extracted)

> Auto-extracted from docs/omegon-install-site.md at decide-time.

## Decisions

### Option A: dedicated nginx pod in brutus cluster (decided)

Matches existing styrene-docs pattern exactly. DNS, TLS, and gateway infrastructure already proven. 32Mi RAM cost is negligible.

### Serve install.sh directly, pinned to image SHA (decided)

install.sh baked into the container image at build time. Deployment references image by SHA digest, not :latest tag. Updates require explicit image bump in the deployment manifest — no drift. Script must be bulletproof: verify checksums, handle partial downloads, clean up on failure, validate extracted binary.

### Just the install script — binaries stay on GitHub Releases (decided)

GitHub Releases provides CDN and bandwidth for tarballs. No need to mirror. install.sh fetches from GitHub Releases API.

## Research Summary

### Existing Infrastructure

**DNS**: Cloudflare-managed via external-dns in brutus k3s cluster. The `external-dns-styrene` app syncs DNS for both `styrene.io` and `styrene.dev` domains. Records created from Gateway API HTTPRoute annotations (`external-dns.alpha.kubernetes.io/cloudflare: "true"`). Default target: `216.82.35.160`.

**TLS**: cert-manager with Let's Encrypt via Cloudflare DNS-01 challenge. Both `styrene.io` and `styrene.dev` wildcards are already configured in the ClusterIssuer. Cloudflare API token stored in …

### Architecture Options

**Option A: Static site in styrene-docs namespace**
Add an nginx container serving a minimal static site (landing page + install.sh). Reuse the same deployment pattern as styrene-docs (GHCR image, namespace, service). New HTTPRoute for `omegon.styrene.dev`. ~5 files in vanderlyn/apps.

Pros: Matches existing patterns exactly. Easy to maintain. Landing page can be a single HTML file.
Cons: Separate container image for a tiny static site. Needs CI to build/push the image.

**Option B: Path-based r…

### Required Changes (Option A)

**1. vanderlyn/apps/envoy-gateway/gateway.yaml**
Add a listener section:
```yaml
- name: https-omegon-styrene-dev
  protocol: HTTPS
  port: 443
  hostname: omegon.styrene.dev
  allowedRoutes:
    namespaces:
      from: Selector
      selector:
        matchLabels:
          gateway/styrene: "true"
  tls:
    certificateRefs:
      - name: styrene-dev-tls
```

**2. vanderlyn/apps/envoy-gateway/httproutes-styrene.yaml**
Add HTTPRoute:
```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRou…
