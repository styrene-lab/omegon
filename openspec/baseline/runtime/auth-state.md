+++
id = "ed661fcb-ca88-4950-92e2-9b72db199db4"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# runtime/auth-state

### Requirement: Installed Omegon preserves existing shared auth state

Installed Omegon must reuse persistent provider auth and other mutable user state from the stable shared user config directory instead of redirecting it into the package install root.

#### Scenario: Existing Omegon auth remains valid across installs
Given a machine already has `~/.config/omegon/auth.json`
And Omegon is installed globally from npm
When the user launches `omegon`
Then Omegon reads auth from the stable shared user config directory
And the user is not required to log in again just because Omegon was installed

#### Scenario: Explicit agent-dir overrides still win
Given `PI_CODING_AGENT_DIR` is explicitly set in the environment
When the user launches `omegon`
Then Omegon respects that explicit override for mutable state paths
And Omegon does not silently replace it with the package install root

### Requirement: Installed Omegon still loads packaged Omegon resources

Omegon must keep its own packaged extensions, skills, and prompt templates active even when mutable state lives under the shared user config directory.

#### Scenario: Packaged resources load without changing agentDir
Given Omegon is installed from npm
When `omegon` starts without an explicit `PI_CODING_AGENT_DIR` override
Then the coding agent loads Omegon-packaged extensions from the installed Omegon root
And it loads Omegon-packaged skills and prompt templates from the installed Omegon root
And mutable state paths still resolve under `~/.config/omegon`

### Requirement: Regressed package-root state is migrated forward safely

Users who already ran a regressed Omegon build that wrote mutable state into the package root must be migrated to the stable shared user config directory without losing credentials.

#### Scenario: Legacy package-root auth is adopted when shared auth is absent
Given a previously installed Omegon package root contains `auth.json`
And the shared user config directory does not yet contain `auth.json`
When the user launches a fixed Omegon build
Then Omegon copies the legacy package-root auth into the shared user config directory
And subsequent launches use the shared user config directory as the canonical state root
And the user is not required to manually move files before logging in
