#!/bin/sh
# Record any lifecycle-owned CLI probe without contacting Vault.
printf '%s\n' "$*" > "${OMEGON_TEST_VAULT_PROBE_MARKER:?}"
printf '%s\n' '{"sealed":true}'
exit 2
