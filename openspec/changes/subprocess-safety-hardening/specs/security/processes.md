# security/processes — Delta Spec

## ADDED Requirements

### Requirement: Browser launch helpers avoid shell-string command construction

Helpers that open browser URLs from pi-kit MUST invoke platform launchers with explicit executable names and argument arrays rather than building shell command strings.

#### Scenario: web-ui opens the browser without a shell string
Given the web-ui extension opens a localhost dashboard URL
When it launches the operator's default browser
Then it invokes the platform launcher with an explicit program and argument list
And it does not pass a shell-formatted command string through `child_process.exec`

#### Scenario: launcher handles platform-specific commands safely
Given pi-kit runs on macOS, Linux, or Windows
When a browser-open helper launches a URL
Then the helper selects the appropriate launcher for that platform
And the URL is passed as a discrete argument rather than interpolated into a shell command

### Requirement: Ollama shutdown avoids broad pkill patterns

The local-inference extension MUST stop the Ollama server using the process handle it started, or a targeted lookup tied to that process, rather than a broad pattern kill that can match unrelated processes.

#### Scenario: stopping managed Ollama terminates only the owned server
Given pi-kit started an Ollama child process during the current session
When the operator requests local inference shutdown
Then pi-kit signals the tracked child process directly
And it does not execute a broad `pkill -f` pattern against all matching processes

#### Scenario: shutdown remains safe when no managed child exists
Given no Ollama child process is currently tracked by pi-kit
When shutdown is requested
Then pi-kit reports that no managed server is running or performs a narrowly scoped fallback
And it does not terminate unrelated Ollama processes opportunistically

### Requirement: Shell-based helper execution is isolated behind reviewed wrappers

When pi-kit must invoke subprocesses for local helper behavior, it MUST prefer explicit command/argv spawning and centralize any unavoidable shell usage behind reviewed wrappers with clear constraints.

#### Scenario: bootstrap helper execution uses explicit command dispatch
Given bootstrap needs to run a local helper command
When it starts the subprocess
Then it uses explicit executable plus argv dispatch where feasible
And any remaining shell-bound execution is isolated so callers do not concatenate arbitrary command fragments

#### Scenario: hardening changes remain regression-tested
Given subprocess/process-management helpers are hardened
When the relevant test suites run
Then browser-launch and local-inference shutdown behavior remain covered by automated tests
And the tests assert the safer execution path instead of shell-string or broad-kill behavior
