# Runtime Node Version Compatibility

### Requirement: Omegon declares the supported Node runtime floor
Omegon MUST declare a root runtime requirement aligned with its vendored pi-mono dependencies.

#### Scenario: Root package declares Node 20+
- **Given** Omegon vendors pi-mono packages that require modern Unicode regex support
- **When** an operator inspects Omegon's root package metadata
- **Then** the root package declares Node 20 or later as the supported runtime floor
- **And** the declared floor matches the practical runtime needs of the bundled dependencies

### Requirement: Unsupported Node runtimes fail early and clearly
Omegon MUST fail before normal startup or install completes on unsupported Node runtimes rather than surfacing a late syntax error from vendored code.

#### Scenario: Preinstall rejects Node 18 with a clear message
- **Given** an operator installs or updates Omegon on Node 18
- **When** the preinstall guard runs
- **Then** the install exits with a clear unsupported-runtime message
- **And** the message directs the operator to upgrade Node instead of suggesting a Unicode/debugging workaround
