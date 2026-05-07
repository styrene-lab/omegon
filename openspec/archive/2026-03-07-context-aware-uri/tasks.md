+++
id = "4611db3b-02d4-4f03-81ab-0de79da806b6"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# context-aware-uri — Tasks

## 1. URI resolution routes by file type and available handlers

- [x] 1.1 Markdown file with mdserve running
- [x] 1.2 Markdown file without mdserve
- [x] 1.3 Code file with editor preference set
- [x] 1.4 Code file with no editor preference
- [x] 1.5 Image file always uses file://
- [x] 1.6 Excalidraw file with Obsidian vault detected
- [x] 1.7 Excalidraw file without Obsidian vault
- [x] 1.8 Write tests for URI resolution routes by file type and available handlers

## 2. mdserve auto-starts on session_start

- [x] 2.1 mdserve binary found on PATH at session start
- [x] 2.2 mdserve binary not found on PATH
- [x] 2.3 mdserve cleaned up on session end
- [x] 2.4 Write tests for mdserve auto-starts on session_start

## 3. Config file at .pi/config.json

- [x] 3.1 Config file with editor preference
- [x] 3.2 Config file missing
- [x] 3.3 Config file with unknown editor
- [x] 3.4 Write tests for Config file at .pi/config.json

## 4. OSC 8 links in view tool output

- [x] 4.1 Header contains clickable link
- [x] 4.2 Terminal without OSC 8 support
- [x] 4.3 Write tests for OSC 8 links in view tool output
