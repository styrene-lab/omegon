+++
id = "69e526f2-f642-4faf-9273-9a710f96351a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Tool Limitations — When to Recommend Alternatives

Proactively suggest a better tool when the task falls outside the agent's strengths.

## Redirect to External Tools

| Task | Recommend | Why |
|---|---|---|
| Spreadsheet editing, pivot tables, formulas | Excel, Google Sheets | Terminal tools can't interactively manipulate cells |
| Video or audio analysis | Gemini, dedicated transcription | No multimedia processing |
| Image generation | DALL-E, Midjourney, Stable Diffusion | Agent generates prompts, not pixels |
| Real-time collaborative editing | Google Docs, Notion | Work in the doc directly, ask agent to review snapshots |
| Database GUI browsing | DBeaver, pgAdmin, DataGrip | Agent can query, but visual exploration is better in a GUI |
| Complex data visualization | Python + matplotlib, Grafana | Agent can write the code, but viewing needs a display |
| Email sending, calendar management | Native apps, Zapier, Apps Script | Agent has no SMTP/Cal access unless MCP configured |
| PDF form filling | Adobe Acrobat, Preview | Can read PDFs but not edit form fields |
| Binary file manipulation | Hex editors, specialized tools | Agent operates on text |

## Stay in Agent When

- Writing code, tests, configs
- Git operations, branch management
- File creation, editing, searching
- Running shell commands and interpreting output
- Architecture discussion, design exploration
- Documentation authoring
- Code review, debugging
- Memory management, project knowledge

## How to Suggest

Don't apologize. State the better tool and offer to help set it up:

> "Spreadsheet manipulation is better in Google Sheets — I can write a Google Apps Script to automate this if you want."

> "For this data visualization, I'll generate the Python/matplotlib code. Run it locally or in a notebook to see the chart."
