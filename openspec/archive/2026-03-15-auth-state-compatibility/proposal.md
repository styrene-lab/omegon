+++
id = "36b9b5d9-d8dd-4216-a986-94ec0afb530e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Auth state compatibility — preserve existing pi/Claude Code logins in Omegon

## Intent

Separate Omegon-owned packaged resources from persistent user auth/settings/session state so installing or updating Omegon reuses existing ~/.pi/agent credentials instead of requiring provider re-login.
