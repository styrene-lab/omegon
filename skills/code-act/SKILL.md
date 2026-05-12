+++
id = "7e1a96a9-576a-4003-9c2f-0890a5e5ae5e"
name = "code-act"
description = "Generate and execute Python scripts instead of sequential tool calls"
tags = ["automation", "scripting", "batch"]
aliases = ["script", "codeact"]
triggers = ["write a script", "batch process", "run a pipeline", "code-act mode"]
+++

# Code-Act Execution Mode

When this skill is active, accomplish tasks by generating complete Python scripts
and executing them, rather than making sequential tool calls.

## When to use code-act

- Batch operations over collections (process all files, review all PRs)
- Data transformation pipelines (read → transform → write)
- Tasks requiring loops, conditionals, or parallel operations
- Tasks where the full plan is known upfront

## Execution pattern

1. Analyze the task and plan the script
2. Write a complete Python script using only the standard library
3. Execute the script via the `bash` tool: `python3 -c '...'` for short scripts,
   or write to a temp file and run `python3 /tmp/script.py` for longer ones
4. Capture and report the output
5. If the script fails, analyze the error and generate a corrected version

## Available helpers in scripts

```python
import subprocess, os, json, sys, glob, pathlib

# Run shell commands
result = subprocess.run(cmd, shell=True, capture_output=True, text=True)

# File I/O
pathlib.Path("output.txt").write_text(content)
content = pathlib.Path("input.txt").read_text()

# Glob patterns
files = sorted(glob.glob("src/**/*.rs", recursive=True))
```

## Rules

- Use only Python standard library (no pip packages)
- Print final results to stdout
- Use `try/except` for error handling
- Prefer `subprocess.run` over `os.system`
- For parallel work, use `concurrent.futures.ThreadPoolExecutor`
- Clean up temp files after execution
- Never use `input()` or interactive prompts
