+++
id = "b422553d-617e-4b9c-b5cb-b902e6e5a082"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fix native dispatch binary resolution — omegon-agent → omegon rename, drop unnecessary --bridge

## Overview

The native cleave child dispatcher in omegon-pi looks for a binary named `omegon-agent` but the Rust binary was renamed to `omegon`. Also passes --bridge unnecessarily — the Rust binary has native Anthropic/OpenAI providers. Also lacks a PATH-based fallback for global npm installs. Fix all three in omegon-pi and publish a patch.

## Open Questions

*No open questions.*
