# inference/discovery — Delta Spec

## ADDED Requirements

### Requirement: Protocol-keyed discovery fetchers

Discovery of live model offerings MUST be implemented per endpoint protocol, not per provider. A single OpenAI-compatible fetcher serves every endpoint whose registry profile declares `openAiCompatible` enumeration; dedicated fetchers exist only where the wire contract differs (openrouter rich metadata, anthropic, google, github-copilot token-exchange). Endpoints without a live enumeration contract declare `discovery: none` and are served entirely by lower inventory layers — non-enumerable is a supported state, not an error.

#### Scenario: OpenAI-compatible endpoint enumerated by the generic fetcher
Given an endpoint with protocol `openAiCompatible` and a resolvable bearer credential
When the discovery refresh runs
Then the generic fetcher issues `GET {baseUrl}/models`
And every returned model id is emitted as an `OfferingPatch` in an `InventoryLayer` with source `Discovery` and evidence recording the endpoint id and fetch timestamp

#### Scenario: GitHub Copilot fetcher reuses the token-exchange transport
Given stored github-copilot credentials that pass token exchange
When the discovery refresh runs
Then the Copilot fetcher obtains a Copilot token and header profile via the shared `github_copilot` transport (not a duplicated implementation)
And the offerings layer contains one entry per model id returned by `{copilot_base_url}/models`

#### Scenario: Non-enumerable provider is untouched by discovery
Given an endpoint whose registry entry declares no discovery contract (e.g. perplexity)
When the discovery refresh runs
Then no fetch is attempted for that endpoint
And its offerings from embedded/manifest layers appear unchanged in the merged snapshot

#### Scenario: Fetcher failure never degrades the snapshot
Given a discovery fetcher that returns a network error or non-2xx status
When the discovery refresh runs
Then the previous last-known-good discovery layer for that endpoint is retained
And the failure is recorded as a diagnostic with the endpoint id and redacted error
And the merged snapshot still validates

### Requirement: Discovered-but-uncurated offerings are ungraded and selectable

A model id returned by discovery that has no embedded or manifest metadata MUST surface as an ungraded offering with conservative defaults (128k context input, 16k output, `coding` capability, no quality grade). Ungraded offerings are excluded from autonomous routing by default and remain explicitly selectable by the operator. Discovery MUST NOT synthesize capability grades.

#### Scenario: Newly shipped provider model appears without registry curation
Given the embedded registry lacks model id `gpt-5.6-sol` for provider github-copilot
And the live Copilot `/models` response includes `gpt-5.6-sol`
When the discovery refresh completes
Then the merged snapshot contains an offering for `github-copilot:gpt-5.6-sol`
And `is_graded()` is false for that offering
And the offering carries the conservative default context limits

#### Scenario: Registry metadata enriches a discovered id
Given the embedded registry contains full metadata for `github-copilot:claude-sonnet-4.6`
And discovery confirms `claude-sonnet-4.6` on the live endpoint
When layers merge
Then the offering carries the registry's context limits, capabilities, and grade
And its availability evidence cites the Discovery source

#### Scenario: Registry-listed model absent from live enumeration
Given the embedded registry lists a model id for an enumerable endpoint
And the live enumeration response does not include that id
When layers merge
Then the offering is marked unavailable-on-endpoint with Discovery evidence
And it is excluded from selection surfaces by default

### Requirement: TTL-cached background refresh with persisted last-known-good

Discovery MUST run asynchronously (startup after credential resolution, explicit operator refresh, TTL expiry) and MUST never block catalog reads on the network. Discovery results MUST persist to a cache file so a fresh process projects the last-known live inventory before any network activity. Provider-supplied expiry (Copilot `expires_at`/`refresh_in`) drives that endpoint's TTL; other endpoints use a configurable default TTL of one hour.

#### Scenario: Fresh process shows cached discovery before network refresh
Given a prior session persisted a discovery cache containing 29 Copilot offerings
When a new process builds its first inventory snapshot before any fetch completes
Then the snapshot includes the 29 cached Copilot offerings with evidence marking them as cached discovery

#### Scenario: TTL expiry triggers refetch, unexpired cache does not
Given a cached discovery layer for an endpoint whose TTL has not expired
When the background refresh cycle runs
Then no fetch is issued for that endpoint
And an endpoint whose TTL has expired is refetched in the same cycle
