# Host authority and shipped content — Delta Spec

Accepted narrow exception to the existing content-pack requirement. This does not
authorize embedding replaceable shipped content into the host.

## MODIFIED Requirements

### Requirement: Shipped content is independently versioned from kernel authority

Replaceable skills, prompt templates, personas, tones, workflows, and catalog
data must be distributed and discoverable as versioned content-pack artifacts
rather than embedded kernel content. The single common constitutional agent
policy is a narrow exception: it is host authority, may be compiled from one
host-owned resource or constant, and cannot be replaced or removed by ordinary
content-pack admission. This exception does not include tool procedures, workflow
bodies, capability/limitation bundles, or persona content.

Installation makes content resident but does not grant callability, trust, host
effects, or prompt admission. A compatible pack may be upgraded or replaced
without rebuilding the kernel artifact. Pack identity/digest checks retain their
existing meaning and must not be described as publisher authentication.

#### Scenario: Content pack is installed
Given a valid shipped or operator-owned content pack is installed
When contribution discovery runs
Then its content identity, version, digest, provenance, and requested capabilities are inventoried
And no executable asset or prompt body is admitted solely because the pack is resident
And self-declared digest validation does not grant publisher authenticity or core authority

#### Scenario: Kernel builds without shipped content
Given fixtures with absent, corrupt, missing-asset, and valid-empty optional content packs
When the constitutional kernel and maintenance artifact are built and started
Then no replaceable shipped skill, prompt template, persona, workflow, or catalog body is embedded or required
And the host-owned common policy remains available with the same core bytes and identity
And optional unavailable content is reported explicitly without erasing required core policy or blocking supported maintenance operations

#### Scenario: Content pack is upgraded independently
Given a compatible newer content-pack artifact is installed
When the next content generation is validated and admitted
Then the pack version and digest change without rebuilding the kernel executable
And existing active sessions retain or migrate content generation only under declared policy
And ordinary replacement content cannot replace or delete the host core or change its authority
