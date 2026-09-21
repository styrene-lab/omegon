# Credential readiness recovery design

Credential resolution keeps explicit non-OAuth environment keys first, then usable stored credentials, usable external credentials, and independent OAuth environment tokens. A known expired token copied into the environment must not regain priority. Fresh external credentials are considered before attempting an expired stored refresh.

Refresh HTTP adapters return a typed, sanitized terminal or transient failure. OAuth invalid_grant and definitive client/authentication rejections are terminal; rate limits, server failures, timeouts, and transport errors are transient. Requests and response bodies are bounded. Unsupported providers and malformed successful responses fail closed.

A process-local coordinator owns one asynchronous gate per canonical provider. Each cached attempt records a digest of the credential generation, never a printable token. Concurrent requests reuse the same successful result or failure. Terminal failures remain suppressed for that generation. Transient failures use a short retry interval. Credential changes automatically invalidate the prior generation; explicit /connect retry clears suppression. A successful refresh remains usable in memory when auth-file write-back fails.

Sync resolution and read-only status discovery apply the same usability rules without performing network calls. Async resolution may refresh only the requested provider. Provider clients must not fall back to a cached OAuth access token after unsuccessful re-resolution. Existing API-key constructor behavior remains supported.

Tests use local HTTP fixtures and temporary auth stores. They verify wire request counts, typed classifications, token precedence, retry suppression, recovery, and absence of inference after rejected OAuth. No provider login or real credential reset is required.

Disconnected startup must not infer a provider from an empty model ID. Model-limit inventory uses synchronous credential inspection, and startup runs it only for a connected nonempty selection. ACP provider-status inventory uses the same read-only resolver. This does not remove startup credential adoption for a genuinely selected saved route: that path may refresh only its requested provider.

Refresh write-back compares the captured stored generation, including absence, while holding the existing auth-file lock. Logout invalidates in-flight refresh coordination. A changed external source observed after HTTP completion supersedes the older refresh result; a usable external credential also precedes cached success from an expired stored grant. This preserves explicit credential replacement and external-tool recovery during concurrent requests.
