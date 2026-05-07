+++
id = "d4516d21-30fd-4265-8bec-382c954fb0e8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# A2A Protocol Integration — Agent-to-Agent interoperability for Omegon — Design Tasks

## 1. Open Questions

- [ ] 1.1 Should A2A be adopted incrementally (server-only first, then client) or as a full bidirectional protocol from the start?
- [ ] 1.2 Should A2A replace the cleave child task-file protocol, or run alongside it as an optional transport?
- [ ] 1.3 What is the security model for an A2A endpoint on a coding agent that handles repo secrets — localhost-only? mTLS? OAuth with what identity provider?
- [ ] 1.4 Does A2A subsume the Omega HTTP API design question, or are they separate concerns (A2A for external interop, bespoke RPC for internal Omega↔Omegon)?
