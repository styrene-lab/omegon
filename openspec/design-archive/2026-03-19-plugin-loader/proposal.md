+++
id = "b29fc496-dab1-4f37-82f2-bcf6f2f939af"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Plugin loader — TOML manifest discovery, HTTP-backed tools and context

## Intent

Implement the plugin loader that reads ~/.omegon/plugins/*/plugin.toml manifests, creates ToolAdapter instances backed by HTTP endpoints, injects context from declared endpoints, and forwards agent events. This is the extension API contract for all external integrations.

See [design doc](../../../docs/plugin-loader.md).
