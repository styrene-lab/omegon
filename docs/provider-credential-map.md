+++
id = "8e641402-3344-4a0e-8a6f-25a16f47fe1d"
kind = "document"
title = "Provider credential authority boundaries"
status = "implemented"
tags = ["auth", "secrets", "providers", "credentials", "reference"]
aliases = ["provider-credential-map"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Provider Credential Map

## Disposition

This document defines credential-authority boundaries. It is not an exhaustive
provider, endpoint, environment-variable, or wire-protocol inventory. Current
provider and credential entries are code-owned and change independently of this
guide.

The canonical routing and lease semantics are in
[Provider contributions and route leases](provider-contributions-and-route-leases.md).

## Authorities

- `core/crates/omegon/src/auth.rs` owns the current credential catalog, storage
  keys, variable-level authentication kinds, login metadata, and
  provider-specific credential probes.
- `core/crates/omegon/src/providers.rs` resolves credentials at the executable
  bridge boundary and reports bounded credential-source classes.
- `core/crates/omegon/src/provider_contributions.rs` declares each executable
  provider's accepted authentication class and validates it against the bridge
  factory.
- `omegon-secrets` and the configured `SecretsManager` own named secret-recipe
  resolution. Unrelated recipes are not searched to make a route work.

## Authentication class versus credential source

Authentication class is contribution metadata: it states whether a factory
accepts an API key, OAuth, either, token exchange, anonymous hosted access, or a
declared local credential posture. It is not a credential and does not identify where one request found
its credential.

Credential-source class is dispatch evidence. The resolver may identify an
environment, stored, external, or secrets-manager source. If a bridge cannot
provide more specific source evidence, route-lease recording may use the
contribution authentication class as the bounded evidence value. Route leases
never contain secret values.

Environment source classes use the authentication kind declared for the
selected provider variable. Variable names do not determine whether a
credential is an API key, OAuth token, or token-exchange credential.

Credential availability is evaluated at execution boundaries and may change.
Do not infer current authentication, refreshability, quota, or route eligibility
from a static catalog row.

The `opencode-zen` contribution uses anonymous hosted access. Selecting a free
model uses the gateway's documented public token and ignores stored API keys.
The ordinary credential inventory does not treat this public route as an
established connection. `/connect free` refreshes eligible models and displays
the applicable data terms before selection. The separate `opencode-go` provider
continues to require its own API key.

## Routing boundary

Credentials do not define fallback compatibility. Interactive fallback is
limited by explicit `fallbackProviders`. Ordinary sessionless resolution may
follow directed compatibility declarations, while exact resolution does not.
An authenticated provider can still be ineligible because no executable
contribution or declared compatible route exists.

Adding a provider is therefore not a one-row credential-map change. It requires
a complete validated provider contribution, an executable factory, matching
authentication semantics, schema/tool behavior, inventory/evidence authority,
and any deliberately directed fallback compatibility.
