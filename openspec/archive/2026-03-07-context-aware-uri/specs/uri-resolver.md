+++
id = "3bb52cd4-c33c-4ec7-8f7c-29c4bf57bb84"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# uri-resolver — Delta Spec

## ADDED Requirements

### Requirement: URI resolution routes by file type and available handlers

`resolveUri(absPath)` returns the best URI for a given file path based on file extension, running services, and user config. `file://` is always the fallback.

#### Scenario: Markdown file with mdserve running

Given mdserve is running on port 3333
And the file is `/Users/dev/project/docs/README.md`
When resolveUri is called
Then the result is `http://localhost:3333/docs/README.md`

#### Scenario: Markdown file without mdserve

Given mdserve is not running
And the file is `/Users/dev/project/docs/README.md`
When resolveUri is called
Then the result is `file:///Users/dev/project/docs/README.md`

#### Scenario: Code file with editor preference set

Given `.pi/config.json` contains `{"editor": "cursor"}`
And the file is `/Users/dev/project/src/index.ts`
When resolveUri is called
Then the result starts with `cursor://file/`

#### Scenario: Code file with no editor preference

Given `.pi/config.json` does not exist or has no `editor` key
And the file is `/Users/dev/project/src/index.ts`
When resolveUri is called
Then the result is `file:///Users/dev/project/src/index.ts`

#### Scenario: Image file always uses file://

Given any configuration state
And the file is `/Users/dev/images/diagram.png`
When resolveUri is called
Then the result is `file:///Users/dev/images/diagram.png`

#### Scenario: Excalidraw file with Obsidian vault detected

Given an Obsidian vault named `notes` contains the file
And the file is `/Users/dev/notes/sketch.excalidraw`
When resolveUri is called
Then the result starts with `obsidian://open?vault=notes`

#### Scenario: Excalidraw file without Obsidian vault

Given no Obsidian vault contains the file
And the file is `/Users/dev/sketch.excalidraw`
When resolveUri is called
Then the result is `file:///Users/dev/sketch.excalidraw`

### Requirement: mdserve auto-starts on session_start

The vault extension starts mdserve automatically when the binary exists on `$PATH`. The process is cleaned up on `session_end`.

#### Scenario: mdserve binary found on PATH at session start

Given `mdserve` is on `$PATH`
And mdserve is not currently running
When `session_start` fires
Then mdserve is spawned serving the project root
And the mdserve port is stored in shared state for uri-resolver to read

#### Scenario: mdserve binary not found on PATH

Given `mdserve` is not on `$PATH`
When `session_start` fires
Then mdserve is not spawned
And no error is shown to the operator

#### Scenario: mdserve cleaned up on session end

Given mdserve was auto-started
When `session_end` fires
Then the mdserve process is killed

### Requirement: Config file at .pi/config.json

Project-local configuration for editor preference and URI scheme overrides.

#### Scenario: Config file with editor preference

Given `.pi/config.json` contains `{"editor": "vscode"}`
When the config is loaded
Then the editor preference is `vscode`

#### Scenario: Config file missing

Given `.pi/config.json` does not exist
When the config is loaded
Then all preferences return defaults (file:// fallback for everything)

#### Scenario: Config file with unknown editor

Given `.pi/config.json` contains `{"editor": "emacs"}`
When resolveUri is called for a code file
Then the result uses `file://` (unknown scheme, safe fallback)

### Requirement: OSC 8 links in view tool output

The view tool's `fileHeader()` wraps the filename in an OSC 8 hyperlink using the resolved URI.

#### Scenario: Header contains clickable link

Given the view tool renders a file
When the header line is emitted
Then the filename is wrapped in `ESC]8;;URI ESC\ text ESC]8;; ESC\` format
And the URI comes from resolveUri

#### Scenario: Terminal without OSC 8 support

Given the terminal does not support OSC 8 hyperlinks
When the header line is rendered
Then the filename text is still visible (OSC 8 sequences are invisible no-ops)
